//! US-15.3 / US-13.5 — campagne adverse des exports du plan de coupe : 180 séquences à graine fixe
//! (poses sur grille de cases de 105 mm, rotations de 90°, miroirs, tailles, défauts conservés ou
//! retirés), chaque état intermédiaire et final étant exporté en CSV, SVG et (une séquence sur
//! trois) PDF, puis **relu par les lecteurs indépendants** de `tests/common`.
//!
//! **Oracle indépendant** : sommets locaux connus, contours posés avec `cos` / `sin`, aire par la
//! formule du lacet, longueur = plus grand `y`, validité décidée seule (bandes, pli, début,
//! recoupement de boîtes), lignes par taille triées (base puis gamme). Contrôles : lignes et
//! cellules du CSV (un plan invalide ou vide = cellules vides), longueurs de tous les traits de
//! coupe du SVG (multiensemble comparé aux côtés de l'oracle), légendes (« rendement » seulement
//! pour un plan valide, « metrage invalide » sinon), PDF : traits à l'échelle 1:1 et légende,
//! projet inchangé (octets). Compteurs asserts. Mutations : voir `docs/JOURNAL.md` (tranche 126).

mod common;
use common::coupe::{mm_pt, read_pdf, read_svg, stroke_len_pt};
use common::csv_reader::parse;
use common::pdf_text::readable;
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::persist;
use pf_core::*;
use pf_export::cutting_export::{cutting_csv, cutting_pdf, cutting_svg};
use pf_export::{PageFormat, PageSetup};
use pf_geometry::PointMm;
use pf_i18n::Lang;

struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.step() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize].clone()
    }
}

type P = (f64, f64);

fn shapes() -> Vec<Vec<P>> {
    vec![
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        vec![(0.0, 0.0), (80.0, 0.0), (0.0, 40.0)],
        vec![
            (0.0, 0.0),
            (80.0, 0.0),
            (80.0, 30.0),
            (30.0, 30.0),
            (30.0, 70.0),
            (0.0, 70.0),
        ],
    ]
}

#[derive(Clone, Copy)]
struct OPl {
    shape: usize,
    size: Option<usize>,
    x: f64,
    y: f64,
    rot: f64,
    mir: bool,
}

fn o_poly(shapes: &[Vec<P>], pl: &OPl) -> Vec<P> {
    let (s, c) = pl.rot.to_radians().sin_cos();
    let (s, c) = (s.round(), c.round());
    shapes[pl.shape]
        .iter()
        .map(|&(x, y)| {
            let x = if pl.mir { -x } else { x };
            (c * x - s * y + pl.x, s * x + c * y + pl.y)
        })
        .collect()
}

fn bbox(v: &[P]) -> (f64, f64, f64, f64) {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in v {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    b
}

fn lace(v: &[P]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

struct OPlan {
    w: f64,
    sel: f64,
    fold: bool,
}

fn o_valid(shapes: &[Vec<P>], plan: &OPlan, state: &[OPl]) -> bool {
    let bands: Vec<(f64, f64)> = if plan.fold {
        vec![(plan.sel, plan.w / 2.0), (plan.w / 2.0, plan.w - plan.sel)]
    } else {
        vec![(plan.sel, plan.w - plan.sel)]
    };
    let boxes: Vec<_> = state.iter().map(|o| bbox(&o_poly(shapes, o))).collect();
    if boxes.iter().any(|b| {
        !bands
            .iter()
            .any(|&(lo, hi)| b.0 >= lo - 1e-6 && b.2 <= hi + 1e-6)
            || b.1 < -1e-6
    }) {
        return false;
    }
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let (a, b) = (boxes[i], boxes[j]);
            if a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3 {
                return false;
            }
        }
    }
    true
}

fn build() -> (Project, Vec<PieceId>, Vec<SizeId>) {
    let defs = shapes();
    let mut p = Project::new("Adverse export plan");
    let s = p.sheets[0].id;
    let mut ids = Vec::new();
    for (k, d) in defs.iter().enumerate() {
        let ox = 400.0 * (k as f64 + 1.0);
        let poly = p
            .apply(Command::AddPolygon {
                sheet: s,
                vertices: d
                    .iter()
                    .map(|v| PointRef::New(PointMm::new(ox + v.0, 30.0 + v.1)))
                    .collect(),
                closed: true,
                role: PathRole::default(),
                layer: None,
            })
            .unwrap();
        let id = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: poly.entities,
                },
                name: format!("F{k}"),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        ids.push(id);
    }
    p.apply(Command::AddSizeRange {
        name: "G".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let sizes = p.sizes[0].sizes.iter().map(|z| z.id).collect();
    (p, ids, sizes)
}

#[derive(Default)]
struct Cov {
    csv_valid: u32,
    csv_invalid: u32,
    csv_empty: u32,
    svg: u32,
    pdf: u32,
    multi_size: u32,
    folded: u32,
}

fn roll() -> PageSetup {
    PageSetup {
        format: PageFormat::Roll { width_mm: 1200.0 },
        ..PageSetup::default()
    }
}

struct Ctx<'a> {
    plan: CuttingPlanId,
    shapes: &'a [Vec<P>],
    oplan: &'a OPlan,
    seq: u32,
}

fn check(p: &Project, c: &Ctx, state: &[OPl], with_pdf: bool, cov: &mut Cov, what: &str) {
    let before = persist::to_json(p).unwrap();
    let valid = !state.is_empty() && o_valid(c.shapes, c.oplan, state);
    let size_label = |i: Option<usize>| {
        i.map_or("taille de base".to_string(), |k| {
            ["S", "M", "L"][k].to_string()
        })
    };
    // CSV.
    let rows = parse(&cutting_csv(p, Lang::Fr).unwrap());
    assert_eq!(rows[0][0], "#metrage", "{what}");
    assert!(rows.len() >= 2, "{what}");
    let data = &rows[1..];
    if state.is_empty() {
        assert_eq!(data.len(), 1, "{what}");
        assert_eq!(data[0][3], "non renseigne", "{what}");
        assert!(data[0][4..].iter().all(String::is_empty), "{what}");
        cov.csv_empty += 1;
    } else if !valid {
        assert_eq!(data.len(), 1, "{what}");
        assert_eq!(data[0][3], "invalide", "{what}");
        assert!(
            data[0][4..].iter().all(String::is_empty),
            "{what} : chiffre sur un plan invalide"
        );
        cov.csv_invalid += 1;
    } else {
        let length = state
            .iter()
            .map(|o| bbox(&o_poly(c.shapes, o)).3)
            .fold(0.0_f64, f64::max);
        let useful = c.oplan.w - 2.0 * c.oplan.sel;
        let total: f64 = state.iter().map(|o| lace(&c.shapes[o.shape])).sum();
        let mut sizes: Vec<Option<usize>> = Vec::new();
        for o in state {
            if !sizes.contains(&o.size) {
                sizes.push(o.size);
            }
        }
        sizes.sort_by_key(|s| s.map_or(0, |i| i + 1));
        assert_eq!(data.len(), sizes.len(), "{what} : une ligne par taille");
        for (row, size) in data.iter().zip(&sizes) {
            let mine: Vec<&OPl> = state.iter().filter(|o| o.size == *size).collect();
            let area: f64 = mine.iter().map(|o| lace(&c.shapes[o.shape])).sum();
            let want = [
                "metrage".to_string(),
                format!("P{}", c.seq),
                "non renseigne".to_string(),
                "valide".to_string(),
                size_label(*size),
                mine.len().to_string(),
                format!("{area:.2}"),
                format!("{length:.2}"),
                format!("{useful:.2}"),
                format!("{:.2}", 100.0 * area / (useful * length)),
                format!("{:.2}", 100.0 * total / (useful * length)),
            ];
            assert_eq!(row.as_slice(), want.as_slice(), "{what}");
        }
        cov.csv_valid += 1;
        cov.multi_size += (sizes.len() > 1) as u32;
        cov.folded += c.oplan.fold as u32;
    }
    if state.is_empty() {
        assert!(cutting_svg(p, c.plan, Lang::Fr).is_err(), "{what}");
        assert!(cutting_pdf(p, c.plan, &roll(), Lang::Fr).is_err(), "{what}");
        return;
    }
    // SVG : multiensemble des longueurs des traits de coupe.
    let svg = String::from_utf8(cutting_svg(p, c.plan, Lang::Fr).unwrap()).unwrap();
    let doc = read_svg(&svg);
    let mut got: Vec<f64> = doc
        .with_role("coupe")
        .iter()
        .map(|i| {
            let (a, b) = (i.pts[0], i.pts[1]);
            ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
        })
        .collect();
    let mut want: Vec<f64> = Vec::new();
    for o in state {
        let v = o_poly(c.shapes, o);
        for i in 0..v.len() {
            let (a, b) = (v[i], v[(i + 1) % v.len()]);
            want.push(((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt());
        }
    }
    got.sort_by(f64::total_cmp);
    want.sort_by(f64::total_cmp);
    assert_eq!(got.len(), want.len(), "{what} : nombre de traits de coupe");
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() < 1e-3, "{what} : {g} / {w}");
    }
    let legend = if valid {
        assert!(svg.contains("rendement "), "{what}");
        let top = state
            .iter()
            .map(|o| bbox(&o_poly(c.shapes, o)).3)
            .fold(0.0_f64, f64::max);
        assert!(
            svg.contains(&format!("longueur {} mm", top as i64)),
            "{what} : cote de longueur"
        );
        assert!(!svg.contains("metrage invalide"), "{what}");
        true
    } else {
        assert!(svg.contains("metrage invalide"), "{what}");
        assert!(
            !svg.contains("rendement"),
            "{what} : rendement chiffré d'un plan invalide"
        );
        false
    };
    assert_eq!(svg.contains(">pli<"), c.oplan.fold, "{what}");
    cov.svg += 1;
    if with_pdf {
        let bytes = cutting_pdf(p, c.plan, &roll(), Lang::Fr).unwrap();
        let text = readable(&bytes);
        assert_eq!(text.contains("rendement "), legend, "{what} légende PDF");
        let strokes = read_pdf(&bytes);
        // Chaque côté de 100 mm (rectangle et carré) se lit à 100 x 72 / 25,4 pt.
        let side = mm_pt(100.0);
        let want100 = want.iter().filter(|w| (**w - 100.0).abs() < 1e-6).count();
        let found = strokes
            .iter()
            .filter(|s| s.dash_pt.is_empty() && (stroke_len_pt(s) - side).abs() < 0.01)
            .count();
        assert!(
            found >= want100,
            "{what} : {found} côtés de 100 mm lus pour {want100}"
        );
        cov.pdf += 1;
    }
    assert_eq!(
        persist::to_json(p).unwrap(),
        before,
        "{what} : projet modifié"
    );
}

#[test]
fn campagne_adverse_exports_du_plan_de_coupe() {
    let defs = shapes();
    let mut cov = Cov::default();
    let mut r = Rng(0x00C0_FFEE_2026_1127);
    for seq in 0..180u32 {
        let (mut p, ids, sizes) = build();
        let oplan = OPlan {
            w: r.pick(&[640.0, 840.0, 1000.0]),
            sel: r.pick(&[0.0, 10.0, 20.0]),
            fold: r.chance(35),
        };
        let plan = p
            .apply(Command::AddCuttingPlan {
                spec: CuttingPlanSpec {
                    name: format!("P{seq}"),
                    material: None,
                    fabric_width_mm: oplan.w,
                    fold: if oplan.fold {
                        FabricFold::Folded
                    } else {
                        FabricFold::Flat
                    },
                    selvedge_mm: oplan.sel,
                    min_gap_mm: 5.0,
                    grain_direction: None,
                },
            })
            .unwrap()
            .cutting_plans[0];
        let ctx = Ctx {
            plan,
            shapes: &defs,
            oplan: &oplan,
            seq,
        };
        let per_row = (((oplan.w - 2.0 * oplan.sel) / 105.0).floor() as u64).max(1);
        let mut state: Vec<OPl> = Vec::new();
        let mut ids_placed: Vec<PlacementId> = Vec::new();
        let mut cell = 0u64;
        let n = 4 + r.below(7);
        for step in 0..n {
            match r.below(10) {
                0..=6 => {
                    let shape = r.below(defs.len() as u64) as usize;
                    let probe = OPl {
                        shape,
                        size: r.pick(&[None, None, Some(0usize), Some(1), Some(2)]),
                        x: 0.0,
                        y: 0.0,
                        rot: r.pick(&[0.0, 90.0, 180.0, 270.0]),
                        mir: r.chance(30),
                    };
                    let b0 = bbox(&o_poly(&defs, &probe));
                    let (tx, ty) = (
                        oplan.sel + 105.0 * (cell % per_row) as f64,
                        105.0 * (cell / per_row) as f64,
                    );
                    cell += 1;
                    let mut o = OPl {
                        x: tx - b0.0,
                        y: ty - b0.1,
                        ..probe
                    };
                    if r.chance(12) {
                        // Défaut conservé : bord, début de l'étoffe ou recoupement.
                        match r.below(3) {
                            0 => o.x = oplan.w - 20.0,
                            1 => o.y = -3.0,
                            _ => {
                                if let Some(first) = state.first() {
                                    o = OPl { shape, ..*first };
                                }
                            }
                        }
                    }
                    let id = p
                        .apply(Command::AddPlacement {
                            plan,
                            piece: ids[o.shape],
                            size: o.size.map(|i| sizes[i]),
                            x_mm: o.x,
                            y_mm: o.y,
                            rotation_deg: o.rot,
                            mirrored: o.mir,
                        })
                        .unwrap()
                        .placements[0];
                    state.push(o);
                    ids_placed.push(id);
                }
                7 | 8 if !state.is_empty() => {
                    let k = r.below(state.len() as u64) as usize;
                    p.apply(Command::RemovePlacement {
                        plan,
                        placement: ids_placed[k],
                    })
                    .unwrap();
                    state.remove(k);
                    ids_placed.remove(k);
                }
                _ => {}
            }
            let last = step + 1 == n;
            if last || r.chance(35) {
                let what = format!("séquence {seq} pas {step}");
                check(&p, &ctx, &state, seq % 3 == 0, &mut cov, &what);
            }
        }
    }
    assert!(cov.csv_valid >= 150, "csv valides {}", cov.csv_valid);
    assert!(cov.csv_invalid >= 25, "csv invalides {}", cov.csv_invalid);
    assert!(cov.csv_empty >= 5, "csv vides {}", cov.csv_empty);
    assert!(cov.svg >= 200, "svg {}", cov.svg);
    assert!(cov.pdf >= 60, "pdf {}", cov.pdf);
    assert!(cov.multi_size >= 50, "plusieurs tailles {}", cov.multi_size);
    assert!(cov.folded >= 40, "plis {}", cov.folded);
}
