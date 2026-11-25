//! US-08.6 — campagne adverse du contrôle des trous de piqûre : 200 séquences à graine fixe sur
//! deux rectangles de longueurs aléatoires (60 à 100 mm, pas de 0,25), lignes au nombre fixé ou au
//! pas, couples déclarés ou reportés, tolérances aléatoires (dont la valeur exacte d'un écart pour
//! tester la borne incluse), annulations et relecture JSON.
//!
//! Oracle **indépendant** : les abscisses des trous sont recalculées ici (bord droit : `i · L / k`),
//! le vis-à-vis attendu est la formule écrite à la main (sens opposés : `L_B · (1 − s / L_A)`), et
//! l'appariement est **vérifié par propriétés** (chaque écart lu = écart recalculé, un-pour-un,
//! statut cohérent avec la tolérance, maximalité : aucun trou sans partenaire ne peut encore en
//! recevoir un à moins de la portée) plus la valeur exacte de chaque trou en trop ou manquant. Un
//! report garde tous les trous appariés à 0,01 mm près quels que soient les deux bords.
//!
//! Propriétés : jamais de panique ; refus = octets identiques ; compteurs prouvant que les contrôles
//! forts se sont exécutés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

const SEQUENCES: usize = 200;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn quarter(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 4.0).round() as usize;
        lo + self.below(n + 1) as f64 / 4.0
    }
}

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    carried: usize,
    declared: usize,
    all_matched_after_carry: usize,
    gap_checks: usize,
    missing_checks: usize,
    extra_checks: usize,
    shifted_seen: usize,
    boundary_checks: usize,
    maximality_checks: usize,
    refused: usize,
    undo_checks: usize,
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn rect_piece(p: &mut Project, s: SheetId, x0: f64, w: f64, name: &str) -> (PieceId, EntityId) {
    p.apply(Command::AddPolygon {
        sheet: s,
        vertices: [(x0, 0.0), (x0 + w, 0.0), (x0 + w, 70.0), (x0, 70.0)]
            .map(|(x, y)| PointRef::New(pt(x, y)))
            .to_vec(),
        closed: true,
        role: PathRole::default(),
        layer: None,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(x0 + w / 2.0, 35.0),
            },
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let sheet = p.sheet(s).unwrap();
    let bottom = sheet
        .entities
        .iter()
        .find(|e| {
            sheet.segment_ends(e.id).is_ok_and(|(a, b)| {
                a.y == 0.0 && b.y == 0.0 && (a.x - x0).abs().min((b.x - x0).abs()) < 1e-9
            })
        })
        .unwrap()
        .id;
    (piece, bottom)
}

/// Abscisses des trous d'une ligne droite de longueur `l` (écrit ici, pas lu du code testé).
fn abscissae(l: f64, pitch: f64, count: Option<u32>) -> Vec<f64> {
    match count {
        Some(1) => vec![l / 2.0],
        Some(n) => (0..n).map(|i| l * i as f64 / (n - 1) as f64).collect(),
        None => {
            let q = l / pitch;
            let k = if (q - q.round()).abs() < 1e-9 {
                q.round()
            } else {
                q.ceil()
            } as usize;
            (0..=k).map(|i| l * i as f64 / k as f64).collect()
        }
    }
}

fn one(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed);
    let (la_len, lb_len) = (rng.quarter(60.0, 100.0), rng.quarter(60.0, 100.0));
    let mut p = Project::new("Adverse controle piqure");
    let s = p.sheets[0].id;
    let (pa, ea) = rect_piece(&mut p, s, 0.0, la_len, "A");
    let (pb, eb) = rect_piece(&mut p, s, 300.0, lb_len, "B");
    let spec = |e: EntityId, pitch: f64, count: Option<u32>| StitchLineSpec {
        edges: vec![e],
        hole: StitchHole::Round { diameter_mm: 1.2 },
        pitch_mm: pitch,
        setback_mm: 3.0,
        count,
        continues: None,
    };
    let (pitch_a, count_a) = (
        rng.quarter(3.0, 12.0),
        rng.chance(50).then(|| 1 + rng.below(14) as u32),
    );
    let la = p
        .apply(Command::AddStitchLine {
            piece: pa,
            line: spec(ea, pitch_a, count_a),
        })
        .unwrap()
        .stitch_lines[0];
    let xa = abscissae(la_len, pitch_a, count_a);
    let mut doc = Document::new(p.clone());
    let original = bytes(doc.project());

    let carry = rng.chance(50);
    let (xb, lb) = if carry {
        let o = doc
            .execute(Command::CarryStitchLine {
                from: StitchRef {
                    piece: pa,
                    line: la,
                },
                to_piece: pb,
                to_edges: vec![eb],
            })
            .unwrap_or_else(|e| panic!("seed {seed} : {e}"));
        c.carried += 1;
        // Même nombre de trous, répartis sur la longueur du bord B.
        let n = xa.len() as u32;
        (abscissae(lb_len, 4.0, Some(n)), o.stitch_lines[0])
    } else {
        let (pitch_b, count_b) = (
            rng.quarter(3.0, 12.0),
            rng.chance(50).then(|| 1 + rng.below(14) as u32),
        );
        let lb = doc
            .execute(Command::AddStitchLine {
                piece: pb,
                line: spec(eb, pitch_b, count_b),
            })
            .unwrap()
            .stitch_lines[0];
        doc.execute(Command::PairStitchLines {
            a: StitchRef {
                piece: pa,
                line: la,
            },
            b: StitchRef {
                piece: pb,
                line: lb,
            },
        })
        .unwrap();
        c.declared += 1;
        (abscissae(lb_len, pitch_b, count_b), lb)
    };
    let project = doc.project().clone();
    // Positions réelles lues dans le modèle : x = x0 + s, y = 3 (retrait).
    let layout_a = project.stitch_line_layout(pa, la).unwrap();
    let layout_b = project.stitch_line_layout(pb, lb).unwrap();
    assert_eq!(layout_a.holes.len(), xa.len());
    assert_eq!(layout_b.holes.len(), xb.len());

    let rep = project.stitch_check_report();
    assert_eq!(rep.len(), 1);
    let r = &rep[0];
    assert!(r.not_computable.is_none());
    assert_eq!((r.count_a, r.count_b), (xa.len(), xb.len()));
    let mean = |x: &[f64]| {
        if x.len() > 1 {
            (x[x.len() - 1] - x[0]) / (x.len() - 1) as f64
        } else {
            0.0
        }
    };
    assert!((r.mean_step_a - mean(&xa)).abs() < 1e-9, "pas moyen A");
    assert!((r.mean_step_b - mean(&xb)).abs() < 1e-9, "pas moyen B");
    assert!((r.step_gap_mm - (mean(&xa) - mean(&xb))).abs() < 1e-9);

    // Vis-à-vis attendus, sens opposés (contours anti-horaires des deux côtés).
    let ea_exp: Vec<f64> = xa.iter().map(|s| lb_len * (1.0 - s / la_len)).collect();
    let eb_exp: Vec<f64> = xb.iter().map(|s| la_len * (1.0 - s / lb_len)).collect();
    let cost = |i: usize, j: usize| (ea_exp[i] - xb[j]).abs().max((eb_exp[j] - xa[i]).abs());
    let tol: f64 = 0.5;
    let half_step = 0.5 * mean(&xa).min(mean(&xb));
    let range = tol.max(half_step);

    let mut used_a = vec![false; xa.len()];
    let mut used_b = vec![false; xb.len()];
    let mut shifted = 0;
    for it in &r.items {
        match (it.a, it.b) {
            (Some(ha), Some(hb)) => {
                let (i, j) = (ha.number as usize - 1, hb.number as usize - 1);
                assert!(!used_a[i] && !used_b[j], "un-pour-un");
                used_a[i] = true;
                used_b[j] = true;
                let want = cost(i, j);
                assert!((it.gap_mm.unwrap() - want).abs() < 1e-6, "écart {i}/{j}");
                assert!(want <= range + 1e-9);
                assert_eq!(it.number, ha.number);
                match it.status {
                    StitchMatch::Matched => assert!(want <= tol + 1e-9),
                    StitchMatch::Shifted => {
                        assert!(want > tol - 1e-9);
                        shifted += 1;
                    }
                    StitchMatch::Missing(_) => panic!("apparié mais manquant"),
                }
                // Positions réelles des deux trous (repère de chaque pièce : abscisse le long du bord).
                assert!((ha.position.x - xa[i]).abs() < 1e-6 && (ha.position.y - 3.0).abs() < 1e-6);
                assert!(
                    (hb.position.x - (300.0 + xb[j])).abs() < 1e-6
                        && (hb.position.y - 3.0).abs() < 1e-6
                );
                c.gap_checks += 1;
            }
            (Some(ha), None) => {
                assert_eq!(
                    it.status,
                    StitchMatch::Missing(pf_core::seam_pair::SeamWhich::B)
                );
                let i = ha.number as usize - 1;
                assert!(!used_a[i]);
                used_a[i] = true;
                assert!(
                    (ha.position.x - xa[i]).abs() < 1e-6,
                    "position du trou manquant"
                );
                c.missing_checks += 1;
            }
            (None, Some(hb)) => {
                assert_eq!(
                    it.status,
                    StitchMatch::Missing(pf_core::seam_pair::SeamWhich::A)
                );
                let j = hb.number as usize - 1;
                assert!(!used_b[j]);
                used_b[j] = true;
                assert!(
                    (hb.position.x - (300.0 + xb[j])).abs() < 1e-6,
                    "position du trou en trop"
                );
                assert!(it.number as usize > xa.len(), "numéros suivant ceux de A");
                c.extra_checks += 1;
            }
            (None, None) => panic!("item vide"),
        }
    }
    assert!(
        used_a.iter().all(|u| *u) && used_b.iter().all(|u| *u),
        "tous les trous listés"
    );
    c.shifted_seen += shifted;
    // Maximalité : aucun trou de A et aucun trou de B laissés sans partenaire ne sont à moins de la
    // portée l'un de l'autre.
    let free_a: Vec<usize> = r
        .items
        .iter()
        .filter(|i| i.status == StitchMatch::Missing(pf_core::seam_pair::SeamWhich::B))
        .map(|i| i.a.unwrap().number as usize - 1)
        .collect();
    let free_b: Vec<usize> = r
        .items
        .iter()
        .filter(|i| i.status == StitchMatch::Missing(pf_core::seam_pair::SeamWhich::A))
        .map(|i| i.b.unwrap().number as usize - 1)
        .collect();
    for &i in &free_a {
        for &j in &free_b {
            assert!(
                cost(i, j) > range - 1e-9,
                "seed {seed} : couple libre {i}/{j}"
            );
        }
    }
    c.maximality_checks += 1;

    // Un report garde chaque trou apparié à 0,01 mm près, quels que soient les deux bords.
    if carry {
        assert!(r.counts_equal());
        assert_eq!(
            r.count_status(StitchMatch::Matched),
            xa.len(),
            "seed {seed}"
        );
        assert!(r.items.iter().all(|i| i.gap_mm.unwrap() <= 0.01));
        c.all_matched_after_carry += 1;
    }
    // Numéros appariés lus sur les deux lignes : une permutation sans doublon, A = 1..n.
    if let (Some(na), Some(nb)) = (
        project.stitch_paired_numbers(pa, la),
        project.stitch_paired_numbers(pb, lb),
    ) {
        assert_eq!(na.len(), xa.len());
        assert_eq!(nb.len(), xb.len());
        let mut sorted = na.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), na.len(), "numéros uniques côté A");
    }

    // Borne incluse : la tolérance égale à un écart exact classe ce couple « apparié ».
    if let Some(it) = r.items.iter().find(|i| i.status == StitchMatch::Shifted) {
        let g = it.gap_mm.unwrap();
        let loose = &project.stitch_check_report_with(g)[0];
        let same = loose
            .items
            .iter()
            .find(|x| x.a.map(|h| h.number) == it.a.map(|h| h.number) && x.b.is_some())
            .unwrap();
        assert_eq!(
            same.status,
            StitchMatch::Matched,
            "seed {seed} : borne incluse"
        );
        c.boundary_checks += 1;
    }

    // Refus : octets identiques.
    let before = bytes(doc.project());
    let bad = Command::PairStitchLines {
        a: StitchRef {
            piece: pa,
            line: la,
        },
        b: StitchRef {
            piece: pb,
            line: lb,
        },
    };
    let res = catch_unwind(AssertUnwindSafe(|| {
        let mut q = doc.project().clone();
        let e = q.apply(bad).expect_err("déjà apparié");
        (e.to_string(), bytes(&q))
    }));
    let (msg, after) = res.expect("pas de panique");
    assert!(!msg.is_empty());
    assert_eq!(before, after);
    assert_eq!(before, bytes(doc.project()));
    c.refused += 1;

    // Annuler / rétablir exacts, JSON identique.
    let done = bytes(doc.project());
    while doc.can_undo() {
        assert!(doc.undo());
    }
    assert_eq!(bytes(doc.project()), original);
    while doc.can_redo() {
        assert!(doc.redo());
    }
    assert_eq!(bytes(doc.project()), done);
    let back = persist::from_json(&done).unwrap();
    assert_eq!(back.stitch_check_report(), rep);
    c.undo_checks += 1;
    c.sequences += 1;
}

#[test]
fn campagne_adverse_controle_des_trous_de_piqure() {
    let mut c = Counters::default();
    for i in 0..SEQUENCES {
        one(0xC0DE_0000 + i as u64 * 104_729, &mut c);
    }
    eprintln!("{c:#?}");
    assert_eq!(c.sequences, SEQUENCES);
    assert!(c.carried >= 60 && c.declared >= 60, "{c:?}");
    assert_eq!(c.all_matched_after_carry, c.carried);
    assert!(c.gap_checks >= 1000, "{c:?}");
    assert!(c.missing_checks >= 100, "{c:?}");
    assert!(c.extra_checks >= 50, "{c:?}");
    assert!(c.shifted_seen >= 50, "{c:?}");
    assert!(c.boundary_checks >= 30, "{c:?}");
    assert_eq!(c.maximality_checks, SEQUENCES);
    assert_eq!(c.refused, SEQUENCES);
    assert_eq!(c.undo_checks, SEQUENCES);
}
