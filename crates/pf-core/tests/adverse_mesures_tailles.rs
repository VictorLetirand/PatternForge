//! US-09.6a — campagne adverse « mesures par taille » : 160 séquences à graine fixe de 14 pas sur
//! une pièce à quatre côtés (quadrilatère convexe aux coins entiers, 3 à 6 tailles, base variable)
//! cousue à un rectangle fixe : écarts posés, remplacés, effacés, changements de base, gamme
//! inconnue.
//!
//! **Oracle indépendant** : le modèle (`Model`) ne garde que la **position absolue de chaque coin
//! dans chaque taille** (base + écart, jamais relue du cœur) et recalcule à la main : longueur de
//! chaque côté (`hypot`), périmètre, aire (formule du lacet), longueur de couture de chaque côté
//! du couple, écart A − B, puis les pas (valeur suivante − précédente), pourcentages, pas moyens,
//! médiane des pas (réécrite : tri puis milieu) et verdict d'irrégularité (signe opposé de deux
//! pas consécutifs, ou écart à la médiane supérieur à 50 % de |médiane|). Un verdict dont la
//! décision est à moins de 1e-6 du seuil (ou un pas non nul à moins de 1e-4 de zéro) est ambigu
//! pour les flottants et n'est pas comparé.
//!
//! Invariants vérifiés à chaque pas : projet valide ; lecture sans effet (octets identiques) ;
//! toutes les cellules, tous les pas, pas moyens et verdicts = oracle ; un changement de base ne
//! change **aucune** cellule ; une gamme inconnue est refusée.
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 98).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const SEQUENCES: u64 = 160;
const STEPS: usize = 14;
const GHOST: u64 = 987_654;
const POOL: [&str; 6] = ["XXS", "XS", "S", "M", "L", "XL"];

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
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
}

type V = (f64, f64);

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Model {
    /// Position absolue du coin `c` dans la taille `s` : `pos[s][c]`.
    pos: Vec<[V; 4]>,
    base: usize,
    /// Côté du quadrilatère cousu au rectangle fixe, et côté de celui-ci (0 = horizontal).
    seam_side: usize,
    other_side_len: f64,
}

const OTHER_W: f64 = 300.0;
const OTHER_H: f64 = 500.0;

fn dist(a: V, b: V) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn shoelace(q: &[V; 4]) -> f64 {
    let mut s = 0.0;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        s += a.0 * b.1 - b.0 * a.1;
    }
    s.abs() / 2.0
}

impl Model {
    fn side(&self, s: usize, i: usize) -> f64 {
        dist(self.pos[s][i], self.pos[s][(i + 1) % 4])
    }

    /// Anti-horaire, angles intérieurs entre 55° et 140°, côtés d'au moins 80 mm dans toutes les
    /// tailles : le quadrilatère reste simple et convexe.
    fn shape_ok(&self) -> bool {
        self.pos.iter().all(|q| {
            (0..4).all(|i| {
                let (p, c, n) = (q[(i + 3) % 4], q[i], q[(i + 1) % 4]);
                let (a, b) = ((p.0 - c.0, p.1 - c.1), (n.0 - c.0, n.1 - c.1));
                let (la, lb) = (a.0.hypot(a.1), b.0.hypot(b.1));
                let cross = b.0 * a.1 - b.1 * a.0;
                let angle = cross.atan2(a.0 * b.0 + a.1 * b.1).to_degrees();
                la >= 80.0 && lb >= 80.0 && (55.0..=140.0).contains(&angle)
            })
        })
    }

    /// Écart `d` du coin `c` dans la taille `s` (≠ base) par rapport à la base.
    fn set(&self, s: usize, c: usize, d: V) -> Option<Model> {
        if s == self.base {
            return None;
        }
        let mut m = self.clone();
        let b = m.pos[m.base][c];
        m.pos[s][c] = (b.0 + d.0, b.1 + d.1);
        Some(m)
    }

    /// Valeurs attendues, par taille, de chaque ligne du tableau.
    #[allow(clippy::vec_init_then_push)]
    fn expected(&self, ids: &Ids) -> Vec<(MeasureKind, Vec<f64>)> {
        let n = self.pos.len();
        let col = |f: &dyn Fn(usize) -> f64| (0..n).map(f).collect::<Vec<f64>>();
        let mut out = Vec::new();
        out.push((
            MeasureKind::PiecePerimeter { piece: ids.piece },
            col(&|s| (0..4).map(|i| self.side(s, i)).sum()),
        ));
        out.push((
            MeasureKind::PieceArea { piece: ids.piece },
            col(&|s| shoelace(&self.pos[s])),
        ));
        out.push((
            MeasureKind::PiecePerimeter { piece: ids.second },
            col(&|_| 2.0 * (OTHER_W + OTHER_H)),
        ));
        out.push((
            MeasureKind::PieceArea { piece: ids.second },
            col(&|_| OTHER_W * OTHER_H),
        ));
        out.push((
            MeasureKind::SeamLength {
                pair: ids.pair,
                side: SeamWhich::A,
            },
            col(&|s| self.side(s, self.seam_side)),
        ));
        out.push((
            MeasureKind::SeamLength {
                pair: ids.pair,
                side: SeamWhich::B,
            },
            col(&|_| self.other_side_len),
        ));
        out.push((
            MeasureKind::SeamGap { pair: ids.pair },
            col(&|s| self.side(s, self.seam_side) - self.other_side_len),
        ));
        for (i, e) in ids.sides.iter().enumerate() {
            out.push((
                MeasureKind::PathLength {
                    sheet: ids.sheet,
                    entity: *e,
                },
                col(&|s| self.side(s, i)),
            ));
        }
        for (i, e) in ids.other_edges.iter().enumerate() {
            let len = if i % 2 == 0 { OTHER_W } else { OTHER_H };
            out.push((
                MeasureKind::PathLength {
                    sheet: ids.sheet,
                    entity: *e,
                },
                col(&|_| len),
            ));
        }
        out
    }
}

/// Médiane réécrite : tri puis milieu.
fn oracle_median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    for i in 0..s.len() {
        for j in 0..s.len() - 1 - i {
            if s[j] > s[j + 1] {
                s.swap(j, j + 1);
            }
        }
    }
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// Verdict attendu : `Some(verdict)` ou `None` si la décision est ambiguë pour les flottants.
fn oracle_alert(values: &[f64]) -> Option<bool> {
    if values.len() < 3 {
        return Some(false);
    }
    let d: Vec<f64> = values.windows(2).map(|w| w[1] - w[0]).collect();
    if d.iter().any(|x| x.abs() > 1e-9 && x.abs() < 1e-4) {
        return None;
    }
    let sign_flip = d
        .windows(2)
        .any(|w| w[0] * w[1] < 0.0 && w[0].abs() > 1e-6 && w[1].abs() > 1e-6);
    let m = oracle_median(&d);
    let threshold = (0.5 * m.abs()).max(1e-6);
    if d.iter()
        .any(|x| ((x - m).abs() - threshold).abs() < 1e-6 && threshold > 1e-6)
    {
        return None;
    }
    let off = d.iter().any(|x| (x - m).abs() > threshold);
    Some(sign_flip || off)
}

// ---------------------------------------------------------------------------------------------
// Projet réel
// ---------------------------------------------------------------------------------------------

struct Ids {
    sheet: SheetId,
    range: SizeRangeId,
    piece: PieceId,
    second: PieceId,
    pair: SeamPairId,
    sides: [EntityId; 4],
    other_edges: Vec<EntityId>,
    corners: [PointId; 4],
    sizes: Vec<SizeId>,
}

fn pt(v: V) -> PointMm {
    PointMm::new(v.0, v.1)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn build(rng: &mut Rng) -> (Project, Ids, Model) {
    let n = 3 + rng.below(4) as usize;
    let labels: Vec<&str> = POOL[POOL.len() - n..].to_vec();
    let base_idx = rng.below(n as u64) as usize;
    let w = rng.range(200, 500) as f64;
    let h = rng.range(200, 500) as f64;
    let j = |r: &mut Rng| r.range(-40, 40) as f64;
    let corners_pos: [V; 4] = [(0.0, 0.0), (w, 0.0), (w + j(rng), h), (j(rng), h)];

    let mut p = Project::new("Adverse mesures par taille");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Gamme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        base: base_idx,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let sizes: Vec<SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let mut sides = Vec::new();
    for i in 0..4 {
        let o = p
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::New(pt(corners_pos[i])),
                to: PointRef::New(pt(corners_pos[(i + 1) % 4])),
            })
            .unwrap();
        sides.push(o.entities[0]);
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: sides.clone(),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let o = p
        .apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(PointMm::new(3000.0, 0.0)),
            width_mm: OTHER_W,
            height_mm: OTHER_H,
        })
        .unwrap();
    let other_edges = o.entities.clone();
    let second = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: other_edges.clone(),
            },
            name: "Dos".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    // L'ordre des tracés du rectangle est lu sur la feuille : horizontaux = largeur.
    let sh = p.sheet(sheet).unwrap();
    let horizontal = |e: EntityId| {
        let (u, v) = sh.segment_ends(e).unwrap();
        (u.y - v.y).abs() < 1e-9
    };
    let mut ordered = other_edges.clone();
    ordered.sort_by_key(|e| !horizontal(*e)); // horizontaux d'abord
    let other_edges = vec![ordered[0], ordered[2], ordered[1], ordered[3]];
    // other_edges[i] : i pair = horizontal (OTHER_W), i impair = vertical (OTHER_H).
    let b_edge_index = rng.below(4) as usize;
    let other_side_len = if b_edge_index % 2 == 0 {
        OTHER_W
    } else {
        OTHER_H
    };
    let seam_side = rng.below(4) as usize;
    let a = p.seam_side_range(piece, sides[seam_side], 1).unwrap();
    let b = p
        .seam_side_range(second, other_edges[b_edge_index], 1)
        .unwrap();
    p.apply(Command::AddSeamPair {
        a,
        b,
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap();
    let pair = p.seams[0].id;
    let sh = p.sheet(sheet).unwrap();
    let corner_ids: Vec<PointId> = corners_pos
        .iter()
        .map(|c| {
            sh.points
                .iter()
                .find(|q| {
                    let pos = sh.position(q.id).unwrap();
                    (pos.x - c.0).abs() < 1e-9 && (pos.y - c.1).abs() < 1e-9
                })
                .expect("coin")
                .id
        })
        .collect();
    let model = Model {
        pos: vec![corners_pos; n],
        base: base_idx,
        seam_side,
        other_side_len,
    };
    let ids = Ids {
        sheet,
        range,
        piece,
        second,
        pair,
        sides: [sides[0], sides[1], sides[2], sides[3]],
        other_edges,
        corners: [corner_ids[0], corner_ids[1], corner_ids[2], corner_ids[3]],
        sizes,
    };
    assert!(model.shape_ok());
    (p, ids, model)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 + 1e-9 * a.abs().max(b.abs())
}

fn check_table(p: &Project, ids: &Ids, m: &Model, ctx: &str) {
    let before = bytes(p);
    let t = p
        .size_measures(ids.range)
        .unwrap_or_else(|e| panic!("{ctx} : {e}"));
    assert_eq!(bytes(p), before, "{ctx} : la lecture a écrit");
    assert_eq!(t.sizes, ids.sizes, "{ctx} : colonnes");
    let want = m.expected(ids);
    assert_eq!(t.rows.len(), want.len(), "{ctx} : nombre de lignes");
    for (kind, vals) in want {
        let row = t
            .rows
            .iter()
            .find(|r| r.kind == kind)
            .unwrap_or_else(|| panic!("{ctx} : ligne absente {kind:?}"));
        assert_eq!(row.values.len(), vals.len(), "{ctx} : {kind:?}");
        for (i, (got, w)) in row.values.iter().zip(&vals).enumerate() {
            let got = got.unwrap_or_else(|| panic!("{ctx} : {kind:?}[{i}] illisible"));
            assert!(close(got, *w), "{ctx} : {kind:?}[{i}] {got} ≠ {w}");
        }
        // Pas, pourcentages, pas moyen.
        assert_eq!(row.steps.len(), vals.len() - 1, "{ctx} : pas de {kind:?}");
        let mut pct_sum = 0.0;
        let mut pct_all = true;
        for (i, w) in vals.windows(2).enumerate() {
            let s = row.steps[i].expect("pas");
            assert!(close(s.delta, w[1] - w[0]), "{ctx} : delta {kind:?}[{i}]");
            match s.percent {
                Some(pc) => {
                    assert!(w[0].abs() > 1e-6, "{ctx} : pourcentage d'une base nulle");
                    assert!(
                        (pc - (w[1] - w[0]) / w[0].abs() * 100.0).abs() <= 1e-6,
                        "{ctx} : % {kind:?}[{i}]"
                    );
                    pct_sum += pc;
                }
                None => {
                    assert!(w[0].abs() <= 1e-6, "{ctx} : pourcentage absent");
                    pct_all = false;
                }
            }
        }
        let mean = (vals[vals.len() - 1] - vals[0]) / (vals.len() - 1) as f64;
        assert!(
            close(row.mean_step.unwrap(), mean),
            "{ctx} : pas moyen {kind:?}"
        );
        if pct_all {
            assert!(
                (row.mean_step_percent.unwrap() - pct_sum / (vals.len() - 1) as f64).abs() <= 1e-6,
                "{ctx} : % moyen {kind:?}"
            );
        }
        if let Some(verdict) = oracle_alert(&vals) {
            assert_eq!(
                row.irregularity.is_some(),
                verdict,
                "{ctx} : alerte {kind:?} sur {vals:?}"
            );
        }
        // Une alerte désigne au moins un pas existant.
        if let Some(irr) = &row.irregularity {
            assert!(!irr.steps.is_empty() && irr.steps.iter().all(|i| *i < row.steps.len()));
        }
    }
}

fn run(seq: u64) -> (usize, usize) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (seq + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
    for _ in 0..4 {
        rng.step();
    }
    let (mut p, ids, mut m) = build(&mut rng);
    check_table(&p, &ids, &m, &format!("seq {seq} départ"));
    let (mut applied, mut alerts) = (0, 0);
    for step in 0..STEPS {
        let ctx = format!("seq {seq} pas {step}");
        match rng.below(10) {
            // Gamme inconnue : refusée, octets identiques.
            0 => {
                let before = bytes(&p);
                assert_eq!(
                    p.size_measures(SizeRangeId(GHOST)),
                    Err(SizeMeasuresError::UnknownRange(SizeRangeId(GHOST))),
                    "{ctx}"
                );
                assert_eq!(bytes(&p), before, "{ctx}");
            }
            // Changement de base : aucune cellule ne change (le modèle garde ses positions).
            1 | 2 => {
                let target = rng.below(m.pos.len() as u64) as usize;
                if p.apply(Command::SetBaseSize {
                    size: ids.sizes[target],
                })
                .is_ok()
                {
                    m.base = target;
                } else {
                    panic!("{ctx} : changement de base refusé");
                }
            }
            // Écart posé, remplacé ou effacé (0, 0).
            _ => {
                let s = rng.below(m.pos.len() as u64) as usize;
                let c = rng.below(4) as usize;
                let d: V = if rng.below(5) == 0 {
                    (0.0, 0.0)
                } else {
                    (rng.range(-25, 25) as f64, rng.range(-25, 25) as f64)
                };
                if let Some(next) = m.set(s, c, d).filter(Model::shape_ok) {
                    p.apply(Command::SetPointGrading {
                        sheet: ids.sheet,
                        point: ids.corners[c],
                        size: ids.sizes[s],
                        dx_mm: d.0,
                        dy_mm: d.1,
                    })
                    .unwrap_or_else(|e| panic!("{ctx} : {e}"));
                    m = next;
                    applied += 1;
                } else if s == m.base {
                    assert!(
                        p.apply(Command::SetPointGrading {
                            sheet: ids.sheet,
                            point: ids.corners[c],
                            size: ids.sizes[s],
                            dx_mm: d.0,
                            dy_mm: d.1,
                        })
                        .is_err(),
                        "{ctx} : écart sur la base accepté"
                    );
                }
            }
        }
        check_table(&p, &ids, &m, &ctx);
    }
    let t = p.size_measures(ids.range).unwrap();
    alerts += t.irregular_rows().count();
    (applied, alerts)
}

#[test]
fn campagne_adverse_mesures_par_taille() {
    let (mut applied, mut alerts) = (0, 0);
    for seq in 0..SEQUENCES {
        let (a, b) = run(seq);
        applied += a;
        alerts += b;
    }
    // La campagne exerce réellement la gradation et l'alerte (pas une campagne vide).
    assert!(applied > 600, "écarts appliqués : {applied}");
    assert!(
        alerts > 20,
        "lignes en alerte en fin de séquence : {alerts}"
    );
}
