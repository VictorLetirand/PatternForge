//! US-15.5 « Cuir » : campagne adverse à graine fixe (180 jeux), **oracle indépendant**.
//!
//! L'oracle ne réutilise rien du code testé : pièces, contour, défauts et zones sont des
//! **rectangles alignés sur les axes** (rotations de 90° en 90°, miroir à 0°), donc toutes les
//! aires sont des produits de longueurs, calculées ici à la main. Sont éprouvés : défaut touché
//! (aire exacte), débordement exact, qualité (pire grade, aires par grade), métrage cuir, refus
//! sans trace (polygone auto-croisé, défaut hors contour ou qui en recoupe un autre), contours
//! minuscules, annulation / rétablissement exacts à l'octet, déterminisme, et propositions
//! automatiques relues par l'oracle (jamais de violation). Les compteurs de couverture sont
//! assertés : une campagne qui n'exerce pas ses cas échoue.

use pf_core::cutting_auto::{AutoError, AutoItem, AutoMode, AutoOptions, UnplaceableReason};
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::hide_layout::{hide_yield, quality_violations};
use pf_core::*;
use pf_geometry::PointMm;
use std::sync::atomic::AtomicBool;

const CASES: u64 = 180;
const SEED: u64 = 0x5eed_c0ff_ee15_0005;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// Entier de `lo` à `hi` inclus, en mm entiers (valeurs exactes en flottant).
    fn int(&mut self, lo: i64, hi: i64) -> f64 {
        (lo + self.below((hi - lo + 1) as u64) as i64) as f64
    }
}

#[derive(Clone, Copy, Debug)]
struct R {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl R {
    fn area(&self) -> f64 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }
    fn inter(&self, o: &R) -> f64 {
        let w = (self.x1.min(o.x1) - self.x0.max(o.x0)).max(0.0);
        let h = (self.y1.min(o.y1) - self.y0.max(o.y0)).max(0.0);
        w * h
    }
    fn pts(&self) -> Vec<PlanePoint> {
        vec![
            PlanePoint {
                x: self.x0,
                y: self.y0,
            },
            PlanePoint {
                x: self.x1,
                y: self.y0,
            },
            PlanePoint {
                x: self.x1,
                y: self.y1,
            },
            PlanePoint {
                x: self.x0,
                y: self.y1,
            },
        ]
    }
    fn contains(&self, o: &R) -> bool {
        o.x0 >= self.x0 && o.x1 <= self.x1 && o.y0 >= self.y0 && o.y1 <= self.y1
    }
}

/// Rectangle posé d'une pièce `w × h` : convention du module `cutting_layout`, recalculée ici.
fn placed(w: f64, h: f64, px: f64, py: f64, rot: u32, mirrored: bool) -> R {
    match (rot, mirrored) {
        (0, false) => R {
            x0: px,
            y0: py,
            x1: px + w,
            y1: py + h,
        },
        (0, true) => R {
            x0: px - w,
            y0: py,
            x1: px,
            y1: py + h,
        },
        (90, _) => R {
            x0: px - h,
            y0: py,
            x1: px,
            y1: py + w,
        },
        (180, _) => R {
            x0: px - w,
            y0: py - h,
            x1: px,
            y1: py,
        },
        (270, _) => R {
            x0: px,
            y0: py - w,
            x1: px + h,
            y1: py,
        },
        _ => unreachable!(),
    }
}

fn piece(p: &mut Project, name: &str, w: f64, h: f64, ox: f64) -> PieceId {
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(ox, 7.0)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    let id = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities,
            },
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetPieceInfo {
        piece: id,
        name: name.into(),
        quantity: 3,
        material: None,
        mirrored_pair: false,
    })
    .unwrap();
    id
}

const DIMS: [(f64, f64); 3] = [(100.0, 100.0), (60.0, 40.0), (30.0, 80.0)];

fn base_project() -> (Project, CuttingPlanId, Vec<PieceId>) {
    let mut p = Project::new("Cuir adverse");
    let ids: Vec<PieceId> = DIMS
        .iter()
        .enumerate()
        .map(|(i, (w, h))| piece(&mut p, &format!("P{i}"), *w, *h, 300.0 + 400.0 * i as f64))
        .collect();
    let plan = p
        .apply(Command::AddCuttingPlan {
            spec: CuttingPlanSpec {
                name: "Peau".into(),
                material: None,
                fabric_width_mm: 1000.0,
                fold: FabricFold::Flat,
                selvedge_mm: 0.0,
                min_gap_mm: 0.0,
                grain_direction: None,
            },
        })
        .unwrap()
        .cutting_plans[0];
    (p, plan, ids)
}

#[derive(Default, Debug)]
struct Cover {
    cases: u32,
    hits: u32,
    overflows: u32,
    multi_grade: u32,
    refused_cross: u32,
    refused_outside: u32,
    refused_defect_overlap: u32,
    tiny: u32,
    on_defect_and_valid: u32,
    yields: u32,
    undo_checks: u32,
    auto_ok: u32,
    auto_no_room: u32,
    determinism: u32,
    quality_violations: u32,
}

fn json(p: &Project) -> String {
    pf_core::persist::to_json(p).unwrap()
}

#[test]
fn campagne_adverse_cuir() {
    let mut rng = Rng(SEED);
    let mut cov = Cover::default();
    let cancel = AtomicBool::new(false);
    for case in 0..CASES {
        cov.cases += 1;
        let (mut p, plan, ids) = base_project();
        let mut doc = Document::new(p.clone());
        let mut snaps = vec![json(doc.project())];

        // Contour : rectangle, parfois minuscule.
        let tiny = rng.below(8) == 0;
        let (cx, cy) = (rng.int(-300, 300), rng.int(-300, 300));
        let (cw, ch) = if tiny {
            cov.tiny += 1;
            (rng.int(1, 40), rng.int(1, 40))
        } else {
            (rng.int(200, 1200), rng.int(150, 900))
        };
        let contour = R {
            x0: cx,
            y0: cy,
            x1: cx + cw,
            y1: cy + ch,
        };

        // Polygone auto-croisé : refusé sans trace.
        if rng.below(3) == 0 {
            let bow = vec![
                PlanePoint { x: cx, y: cy },
                PlanePoint {
                    x: cx + cw,
                    y: cy + ch,
                },
                PlanePoint { x: cx + cw, y: cy },
                PlanePoint { x: cx, y: cy + ch },
            ];
            let before = json(doc.project());
            assert!(doc
                .execute(Command::SetHideContour { plan, contour: bow })
                .is_err());
            assert_eq!(
                json(doc.project()),
                before,
                "cas {case} : trace après refus"
            );
            cov.refused_cross += 1;
        }
        doc.execute(Command::SetHideContour {
            plan,
            contour: contour.pts(),
        })
        .unwrap();
        snaps.push(json(doc.project()));

        // Défauts : rectangles dans le contour, sans recoupement ; essais refusés comptés.
        let mut defects: Vec<(HideDefectId, R)> = Vec::new();
        for _ in 0..rng.below(4) {
            let (dw, dh) = (rng.int(1, 120).min(cw), rng.int(1, 120).min(ch));
            let (dx, dy) = (
                cx + rng.int(0, (cw - dw) as i64),
                cy + rng.int(0, (ch - dh) as i64),
            );
            let r = R {
                x0: dx,
                y0: dy,
                x1: dx + dw,
                y1: dy + dh,
            };
            let overlaps = defects.iter().any(|(_, o)| o.inter(&r) > 0.01);
            let res = doc.execute(Command::AddHideDefect {
                plan,
                kind: DefectKind::Hole,
                polygon: r.pts(),
                note: None,
            });
            if overlaps {
                assert!(res.is_err(), "cas {case} : défaut recoupant accepté");
                cov.refused_defect_overlap += 1;
            } else {
                defects.push((res.unwrap().hide_defects[0], r));
                snaps.push(json(doc.project()));
            }
        }
        // Défaut hors du contour : refusé sans trace.
        if rng.below(3) == 0 {
            let r = R {
                x0: cx + cw - 1.0,
                y0: cy,
                x1: cx + cw + 20.0,
                y1: cy + 5.0,
            };
            let before = json(doc.project());
            assert!(doc
                .execute(Command::AddHideDefect {
                    plan,
                    kind: DefectKind::Stain,
                    polygon: r.pts(),
                    note: None,
                })
                .is_err());
            assert_eq!(json(doc.project()), before);
            cov.refused_outside += 1;
        }
        // Zones de qualité (peuvent se recouvrir).
        let mut zones: Vec<(u8, R)> = Vec::new();
        for _ in 0..rng.below(4) {
            let (zx, zy) = (cx + rng.int(0, cw as i64), cy + rng.int(0, ch as i64));
            let r = R {
                x0: zx - rng.int(0, 200),
                y0: zy - rng.int(0, 200),
                x1: zx + rng.int(1, 300),
                y1: zy + rng.int(1, 300),
            };
            let grade = 1 + rng.below(3) as u8;
            doc.execute(Command::AddQualityZone {
                plan,
                grade,
                polygon: r.pts(),
            })
            .unwrap();
            zones.push((grade, r));
            snaps.push(json(doc.project()));
        }
        // Poses.
        let mut poses: Vec<(PlacementId, usize, R)> = Vec::new();
        for _ in 0..(1 + rng.below(4)) {
            let k = rng.below(3) as usize;
            let (w, h) = DIMS[k];
            let rot = [0u32, 90, 180, 270][rng.below(4) as usize];
            let mirrored = rot == 0 && rng.below(4) == 0;
            let on_defect = !defects.is_empty() && rng.below(2) == 0;
            let (px, py) = if on_defect {
                let d = defects[rng.below(defects.len() as u64) as usize].1;
                (d.x0 + rng.int(-50, 50), d.y0 + rng.int(-50, 50))
            } else {
                (
                    cx + rng.int(-60, cw as i64 + 60),
                    cy + rng.int(-60, ch as i64 + 60),
                )
            };
            let id = doc
                .execute(Command::AddPlacement {
                    plan,
                    piece: ids[k],
                    size: None,
                    x_mm: px,
                    y_mm: py,
                    rotation_deg: f64::from(rot),
                    mirrored,
                })
                .unwrap()
                .placements[0];
            poses.push((id, k, placed(w, h, px, py, rot, mirrored)));
            snaps.push(json(doc.project()));
        }
        p = doc.project().clone();

        // --- Diagnostics contre l'oracle ---
        let an = p.analyze_cutting_plan(plan).unwrap();
        let an2 = p.analyze_cutting_plan(plan).unwrap();
        assert_eq!(an, an2, "cas {case} : déterminisme");
        cov.determinism += 1;
        let mut any_hit = false;
        let mut any_over = false;
        for (id, _k, r) in &poses {
            // Défauts touchés.
            for (did, d) in &defects {
                let a = r.inter(d);
                let got = an
                    .defect_hits
                    .iter()
                    .find(|h| h.placement == *id && h.defect == *did);
                if a > 0.01 {
                    let h = got.unwrap_or_else(|| panic!("cas {case} : défaut manqué"));
                    assert!(
                        (h.area_mm2 - a).abs() < 1e-6,
                        "cas {case} : aire {} vs {a}",
                        h.area_mm2
                    );
                    any_hit = true;
                } else {
                    assert!(got.is_none(), "cas {case} : faux défaut touché");
                }
            }
            // Débordement.
            let out = r.area() - r.inter(&contour);
            let got = an.hide_overflows.iter().find(|o| o.placement == *id);
            if out > 0.01 {
                let o = got.unwrap_or_else(|| panic!("cas {case} : débordement manqué"));
                assert!(
                    (o.area_mm2 - out).abs() < 1e-6,
                    "cas {case} : débord {} vs {out}",
                    o.area_mm2
                );
                any_over = true;
            } else {
                assert!(got.is_none(), "cas {case} : faux débordement");
            }
            // Qualité.
            let mut areas = [0.0_f64; 3];
            let mut worst: Option<u8> = None;
            for (g, z) in &zones {
                let a = r.inter(z);
                if a > 0.01 {
                    areas[usize::from(*g) - 1] += a;
                    worst = Some(worst.map_or(*g, |w: u8| w.max(*g)));
                }
            }
            let q = an.qualities.iter().find(|q| q.placement == *id).unwrap();
            assert_eq!(q.worst_grade, worst, "cas {case} : pire grade");
            for (i, (got, want)) in q.areas_mm2.iter().zip(&areas).enumerate() {
                assert!((got - want).abs() < 1e-6, "cas {case} : aire grade {i}");
            }
            if areas.iter().filter(|a| **a > 0.0).count() >= 2 {
                cov.multi_grade += 1;
            }
        }
        cov.hits += u32::from(any_hit);
        cov.overflows += u32::from(any_over);
        // Exigence de qualité : cohérente avec l'oracle.
        for req in 1..=3u8 {
            let want: Vec<PlacementId> = poses
                .iter()
                .filter(|(id, ..)| {
                    an.qualities
                        .iter()
                        .find(|q| q.placement == *id)
                        .and_then(|q| q.worst_grade)
                        .is_some_and(|g| g > req)
                })
                .map(|(id, ..)| *id)
                .collect();
            let got = quality_violations(&an, req);
            assert_eq!(got, want);
            cov.quality_violations += u32::from(!got.is_empty());
        }
        // Métrage : seulement si valide ; valeur calculée à la main.
        let hy = hide_yield(p.cutting_plan(plan).unwrap().hide.as_ref(), &an);
        let valid = !any_hit && !any_over && {
            // Chevauchements entre poses (pièces sans écart).
            let mut ok = true;
            for i in 0..poses.len() {
                for j in (i + 1)..poses.len() {
                    if poses[i].2.inter(&poses[j].2) > 0.01 {
                        ok = false;
                    }
                }
            }
            ok
        };
        assert_eq!(an.is_valid(), valid, "cas {case} : validité");
        match hy {
            Some(y) => {
                assert!(valid);
                let defects_area: f64 = defects.iter().map(|(_, d)| d.area()).sum();
                let useful = contour.area() - defects_area;
                let used: f64 = poses.iter().map(|(_, _, r)| r.area()).sum();
                assert!((y.useful_area_mm2 - useful).abs() < 1e-6);
                assert!((y.used_area_mm2 - used).abs() < 1e-6);
                assert!((y.percent - 100.0 * used / useful).abs() < 1e-9);
                cov.yields += 1;
                if !defects.is_empty() {
                    cov.on_defect_and_valid += 1;
                }
            }
            None => assert!(!valid, "cas {case} : métrage manquant sur plan valide"),
        }

        // --- Annulation / rétablissement exacts à l'octet ---
        let steps = snaps.len() - 1;
        for back in (0..steps).rev() {
            assert!(doc.undo(), "cas {case} : annulation {back}");
            assert_eq!(
                json(doc.project()),
                snaps[back],
                "cas {case} : annulé à {back}"
            );
            cov.undo_checks += 1;
        }
        for (fwd, snap) in snaps.iter().enumerate().skip(1) {
            assert!(doc.redo());
            assert_eq!(&json(doc.project()), snap, "cas {case} : rétabli à {fwd}");
        }
        assert_eq!(json(doc.project()), json(&p));

        // --- Placement automatique relu par l'oracle (plans valides, sans poses) ---
        if case % 3 == 0 {
            let mut empty = p.clone();
            for (id, ..) in &poses {
                empty
                    .apply(Command::RemovePlacement {
                        plan,
                        placement: *id,
                    })
                    .unwrap();
            }
            let items: Vec<AutoItem> = (0..3)
                .map(|k| AutoItem {
                    piece: ids[k],
                    size: None,
                    quantity: 1 + (case as u32 + k as u32) % 2,
                    mirrored_quantity: 0,
                })
                .collect();
            let opts = AutoOptions::default();
            let run = |q: &Project| q.propose_cutting_layout(plan, &items, &opts, &cancel);
            let first = run(&empty);
            let again = run(&empty);
            assert_eq!(first, again, "cas {case} : proposition non déterministe");
            match first {
                Ok(prop) => {
                    let mut done = empty.clone();
                    done.apply(prop.command).unwrap();
                    let rects: Vec<R> = done
                        .cutting_plan(plan)
                        .unwrap()
                        .placements
                        .iter()
                        .map(|pl| {
                            let k = ids.iter().position(|i| *i == pl.piece).unwrap();
                            placed(
                                DIMS[k].0,
                                DIMS[k].1,
                                pl.x_mm,
                                pl.y_mm,
                                pl.rotation_deg as u32,
                                pl.mirrored,
                            )
                        })
                        .collect();
                    let want: u32 = items.iter().map(|i| i.quantity).sum();
                    assert_eq!(rects.len() as u32, want, "cas {case} : quantités");
                    for (i, r) in rects.iter().enumerate() {
                        assert!(
                            contour.contains(r),
                            "cas {case} : proposition hors contour {r:?}"
                        );
                        assert!(
                            defects.iter().all(|(_, d)| r.inter(d) <= 0.01),
                            "cas {case} : défaut touché"
                        );
                        for o in &rects[(i + 1)..] {
                            assert!(r.inter(o) <= 0.01, "cas {case} : chevauchement");
                        }
                    }
                    cov.auto_ok += 1;
                }
                Err(AutoError::Unplaceable(list)) => {
                    assert!(!list.is_empty());
                    assert!(list
                        .iter()
                        .all(|u| matches!(u.reason, UnplaceableReason::NoRoomOnHide)));
                    cov.auto_no_room += 1;
                }
                Err(e) => panic!("cas {case} : refus inattendu {e:?}"),
            }
        }
    }
    eprintln!("{cov:?}");
    assert_eq!(cov.cases, CASES as u32);
    assert!(cov.hits >= 20, "pose sur défaut peu couverte : {cov:?}");
    assert!(cov.overflows >= 30, "débordement peu couvert : {cov:?}");
    assert!(cov.multi_grade >= 5, "multi-grades peu couvert : {cov:?}");
    assert!(cov.refused_cross >= 30, "auto-croisé peu couvert : {cov:?}");
    assert!(
        cov.refused_outside >= 30,
        "défaut hors contour peu couvert : {cov:?}"
    );
    assert!(
        cov.refused_defect_overlap >= 1,
        "défauts recoupés non couverts : {cov:?}"
    );
    assert!(cov.tiny >= 10, "contours minuscules peu couverts : {cov:?}");
    assert!(
        cov.yields >= 10 && cov.on_defect_and_valid >= 3,
        "métrage peu couvert : {cov:?}"
    );
    assert!(
        cov.quality_violations >= 5,
        "exigence de qualité peu couverte : {cov:?}"
    );
    assert!(
        cov.auto_ok >= 10,
        "proposition réussie peu couverte : {cov:?}"
    );
    assert!(
        cov.auto_no_room >= 3,
        "proposition refusée peu couverte : {cov:?}"
    );
    assert!(cov.undo_checks >= 400, "annulation peu couverte : {cov:?}");
}

#[test]
fn mode_completer_sur_peau_conserve_l_existant() {
    let (mut p, plan, ids) = base_project();
    p.apply(Command::SetHideContour {
        plan,
        contour: R {
            x0: 0.0,
            y0: 0.0,
            x1: 400.0,
            y1: 100.0,
        }
        .pts(),
    })
    .unwrap();
    p.apply(Command::AddHideDefect {
        plan,
        kind: DefectKind::Scar,
        polygon: R {
            x0: 100.0,
            y0: 0.0,
            x1: 150.0,
            y1: 100.0,
        }
        .pts(),
        note: None,
    })
    .unwrap();
    let pl = p
        .apply(Command::AddPlacement {
            plan,
            piece: ids[0],
            size: None,
            x_mm: 0.0,
            y_mm: 0.0,
            rotation_deg: 0.0,
            mirrored: false,
        })
        .unwrap()
        .placements[0];
    let cancel = AtomicBool::new(false);
    let items = vec![AutoItem {
        piece: ids[0],
        size: None,
        quantity: 2,
        mirrored_quantity: 0,
    }];
    let prop = p
        .propose_hide_layout(
            plan,
            &items,
            &AutoOptions::default(),
            None,
            AutoMode::CompleteMissing,
            &cancel,
        )
        .unwrap();
    let mut done = p.clone();
    done.apply(prop.command).unwrap();
    let plan_ref = done.cutting_plan(plan).unwrap();
    assert_eq!(plan_ref.placements.len(), 2);
    assert_eq!(
        plan_ref.placements[0].id, pl,
        "l'existant reste le premier, intact"
    );
    assert_eq!(plan_ref.placements[0].x_mm, 0.0);
    // Posée à droite du défaut : de 150 à 250.
    let second = plan_ref.placements[1];
    assert!(
        second.x_mm >= 150.0 - 1e-9 && second.x_mm + 100.0 <= 400.0 + 1e-9,
        "{second:?}"
    );
    assert!(done.analyze_cutting_plan(plan).unwrap().is_valid());
}

#[test]
fn aucun_changement_sans_peau_octets_identiques() {
    // Un plan sans peau : mêmes diagnostics qu'avant (aucun champ cuir rempli).
    let (mut p, plan, ids) = base_project();
    p.apply(Command::AddPlacement {
        plan,
        piece: ids[0],
        size: None,
        x_mm: 10.0,
        y_mm: 10.0,
        rotation_deg: 0.0,
        mirrored: false,
    })
    .unwrap();
    let an = p.analyze_cutting_plan(plan).unwrap();
    assert!(an.defect_hits.is_empty() && an.hide_overflows.is_empty() && an.qualities.is_empty());
    assert!(!json(&p).contains("\"hide\""));
}
