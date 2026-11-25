//! US-15.5 — placement automatique **sur peau** : proposition dédiée aux plans qui ont une peau
//! (les plans sans peau gardent `propose_cutting_layout_mode` tel quel).
//!
//! Une seule passe, déterministe, sans glissement (pas de niveau 2) : les instances sont triées
//! par aire décroissante ; chacune prend la **première** position valide dans l'ordre (bas, puis
//! gauche) parmi des positions candidates alignées sur les sommets du contour, les boîtes des
//! défauts et des pièces déjà posées. Une position est valide si le **polygone exact** de la
//! pièce reste dans le contour (débordement ≤ [`OVERLAP_MIN_MM2`]), ne touche aucun défaut,
//! ne recoupe aucune pièce, respecte l'écart minimal et l'exigence de qualité éventuelle (`min_grade` :
//! aucune aire dans une zone de grade pire). La boîte englobante ne sert qu'à écarter les
//! candidates ; le test final est exact. La proposition est ensuite **relue** (copie du projet,
//! diagnostics complets, quantités) : jamais de proposition qui viole un diagnostic.
//!
//! Limites : pas d'optimisation (premier emplacement valide, ni optimal ni garanti minimal) ;
//! la laize, les lisières et le pli ne comptent pas (le contour les remplace) ; l'exigence de
//! qualité s'applique à toutes les pièces du plan.

use super::*;
use crate::cutting_layout::{TriSet, OVERLAP_MIN_MM2};
use crate::hide_layout::{ccw, hide_yield, quality_violations};

fn rect_overlap(a: (PointMm, PointMm), b: (PointMm, PointMm), margin: f64) -> bool {
    a.0.x <= b.1.x + margin
        && b.0.x <= a.1.x + margin
        && a.0.y <= b.1.y + margin
        && b.0.y <= a.1.y + margin
}

struct Obstacle {
    min: PointMm,
    max: PointMm,
    poly: Vec<PointMm>,
    tris: TriSet,
}

fn obstacle(poly: Vec<PointMm>) -> Obstacle {
    let (min, max) = bounds(&poly);
    let tris = TriSet::new(&poly);
    Obstacle {
        min,
        max,
        poly,
        tris,
    }
}

fn uniq_sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.retain(|x| x.is_finite());
    v.sort_by(|a, b| a.total_cmp(b));
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    v
}

impl Project {
    /// Proposition sur peau (voir le module). `min_grade` : grade maximal accepté (1 à 3), `None`
    /// = aucune exigence.
    pub fn propose_hide_layout(
        &self,
        plan: CuttingPlanId,
        items: &[AutoItem],
        options: &AutoOptions,
        min_grade: Option<u8>,
        mode: AutoMode,
        cancel: &AtomicBool,
    ) -> Result<AutoProposal, AutoError> {
        let deadline = Instant::now() + Duration::from_millis(options.time_limit_ms);
        let plan_ref = self.cutting_plan(plan)?;
        let hide = plan_ref
            .hide
            .as_ref()
            .ok_or_else(|| AutoError::Core(CoreError::Invalid("le plan n'a pas de peau".into())))?;
        let gap = plan_ref.min_gap_mm;
        let keep = mode == AutoMode::CompleteMissing && !plan_ref.placements.is_empty();
        let kept: Vec<CuttingPlacement> = if keep {
            plan_ref.placements.clone()
        } else {
            Vec::new()
        };
        cancelled(cancel)?;
        let complete = if keep {
            if items.iter().all(|i| i.quantity + i.mirrored_quantity == 0) {
                return Err(AutoError::NothingToPlace);
            }
            let found = self.missing_items(plan, items)?;
            if found.0.is_empty() {
                return Err(AutoError::NothingToComplete);
            }
            Some(found)
        } else {
            None
        };
        let items = complete.as_ref().map_or(items, |c| &c.0[..]);
        let total: usize = items
            .iter()
            .map(|i| i.quantity as usize + i.mirrored_quantity as usize)
            .sum();
        if total == 0 {
            return Err(AutoError::NothingToPlace);
        }
        if total > MAX_INSTANCES {
            return Err(AutoError::TooMany(total));
        }

        // Variantes (pièce, taille, miroir) et leurs rotations permises.
        struct HVariant {
            piece: PieceId,
            size: Option<SizeId>,
            mirrored: bool,
            shape: PieceShape,
            rots: Vec<(f64, PointMm, f64, f64)>,
        }
        let mut variants: Vec<HVariant> = Vec::new();
        let mut inst_var: Vec<usize> = Vec::new();
        let mut expected: HashMap<(PieceId, Option<SizeId>, bool), usize> = HashMap::new();
        let mut bad: Vec<Unplaceable> = Vec::new();
        for item in items {
            let piece = self.piece(item.piece)?;
            if item.mirrored_quantity > 0 && !piece.mirrored_pair {
                return Err(AutoError::MirrorNotAllowed(item.piece));
            }
            for (mirrored, n) in [(false, item.quantity), (true, item.mirrored_quantity)] {
                if n == 0 {
                    continue;
                }
                let vi = match variants.iter().position(|v| {
                    (v.piece, v.size, v.mirrored) == (item.piece, item.size, mirrored)
                }) {
                    Some(vi) => vi,
                    None => {
                        let shape = match self.piece_shape(item.piece, item.size) {
                            Ok(s) => s,
                            Err(e) => {
                                if !bad
                                    .iter()
                                    .any(|b| (b.piece, b.size) == (item.piece, item.size))
                                {
                                    bad.push(Unplaceable {
                                        piece: item.piece,
                                        size: item.size,
                                        reason: UnplaceableReason::Unreadable(e.to_string()),
                                    });
                                }
                                continue;
                            }
                        };
                        let degs =
                            rotation_candidates(plan_ref, &shape, mirrored, options.allow_rotation);
                        if degs.is_empty() {
                            if !bad
                                .iter()
                                .any(|b| (b.piece, b.size) == (item.piece, item.size))
                            {
                                bad.push(Unplaceable {
                                    piece: item.piece,
                                    size: item.size,
                                    reason: UnplaceableReason::NoRotation,
                                });
                            }
                            continue;
                        }
                        let rots = degs
                            .iter()
                            .map(|d| {
                                let poly = place_polygon(&shape.polygon, 0.0, 0.0, *d, mirrored);
                                let (lo, hi) = bounds(&poly);
                                (*d, lo, hi.x - lo.x, hi.y - lo.y)
                            })
                            .collect();
                        variants.push(HVariant {
                            piece: item.piece,
                            size: item.size,
                            mirrored,
                            shape,
                            rots,
                        });
                        variants.len() - 1
                    }
                };
                *expected
                    .entry((item.piece, item.size, mirrored))
                    .or_insert(0) += n as usize;
                inst_var.extend(std::iter::repeat_n(vi, n as usize));
            }
        }
        if !bad.is_empty() {
            return Err(AutoError::Unplaceable(bad));
        }
        let expected = complete.as_ref().map_or(&expected, |c| &c.2).clone();

        // Obstacles fixes : contour, défauts, zones à éviter selon l'exigence, pièces conservées.
        let contour = obstacle(ccw(&hide.contour));
        let defects: Vec<Obstacle> = hide
            .defects
            .iter()
            .map(|d| obstacle(ccw(&d.polygon)))
            .collect();
        let bad_zones: Vec<Obstacle> = match min_grade {
            Some(g) => hide
                .quality_zones
                .iter()
                .filter(|z| z.grade > g)
                .map(|z| obstacle(ccw(&z.polygon)))
                .collect(),
            None => Vec::new(),
        };
        let mut placed: Vec<Obstacle> = complete
            .as_ref()
            .map(|c| c.1.iter().map(|f| obstacle(f.poly.clone())).collect())
            .unwrap_or_default();
        let n_fixed = placed.len();

        let mut order: Vec<usize> = (0..inst_var.len()).collect();
        order.sort_by(|a, b| {
            variants[inst_var[*b]]
                .shape
                .area_mm2
                .total_cmp(&variants[inst_var[*a]].shape.area_mm2)
                .then(a.cmp(b))
        });
        let mut poses: Vec<Option<AutoPose>> = vec![None; inst_var.len()];
        let mut stuck: Vec<Unplaceable> = Vec::new();
        for inst in order {
            let v = &variants[inst_var[inst]];
            let mut best: Option<(f64, f64, AutoPose, Vec<PointMm>)> = None;
            for &(deg, lo, w, h) in &v.rots {
                cancelled(cancel)?;
                if Instant::now() >= deadline {
                    return Err(AutoError::VerificationFailed(
                        "borne de temps atteinte avant d'avoir posé toutes les pièces".into(),
                    ));
                }
                let base = place_polygon(&v.shape.polygon, 0.0, 0.0, deg, v.mirrored);
                let (cmin, cmax) = (contour.min, contour.max);
                let mut xs = vec![cmin.x, cmax.x - w];
                let mut ys = vec![cmin.y, cmax.y - h];
                for p in &contour.poly {
                    xs.extend([p.x, p.x - w]);
                    ys.extend([p.y, p.y - h]);
                }
                for o in defects.iter().chain(&bad_zones) {
                    xs.extend([o.max.x, o.min.x - w]);
                    ys.extend([o.max.y, o.min.y - h]);
                }
                for o in &placed {
                    xs.extend([o.max.x + gap, o.min.x - w - gap]);
                    ys.extend([o.max.y + gap, o.min.y - h - gap]);
                }
                let xs: Vec<f64> = uniq_sorted(xs)
                    .into_iter()
                    .filter(|x| *x >= cmin.x - EPS && x + w <= cmax.x + EPS)
                    .collect();
                let ys: Vec<f64> = uniq_sorted(ys)
                    .into_iter()
                    .filter(|y| *y >= cmin.y - EPS && y + h <= cmax.y + EPS)
                    .collect();
                'search: for &by in &ys {
                    if best.as_ref().is_some_and(|b| by > b.0 + EPS) {
                        break;
                    }
                    for &bx in &xs {
                        let rect = (PointMm::new(bx, by), PointMm::new(bx + w, by + h));
                        let (px, py) = (bx - lo.x, by - lo.y);
                        let poly: Vec<PointMm> = base
                            .iter()
                            .map(|p| PointMm::new(p.x + px, p.y + py))
                            .collect();
                        let me = TriSet::new(&poly);
                        let hits = |list: &[Obstacle]| {
                            list.iter().any(|o| {
                                rect_overlap(rect, (o.min, o.max), 0.0)
                                    && me.intersection_area(&o.tris) > OVERLAP_MIN_MM2
                            })
                        };
                        // Pièces posées : chevauchement exact, puis écart minimal.
                        let clash = placed.iter().any(|o| {
                            if !rect_overlap(rect, (o.min, o.max), gap) {
                                return false;
                            }
                            me.intersection_area(&o.tris) > OVERLAP_MIN_MM2
                                || (gap > 0.0 && polygon_distance_mm(&poly, &o.poly) < gap - EPS)
                        });
                        if clash || hits(&defects) || hits(&bad_zones) {
                            continue;
                        }
                        let area = crate::cutting_layout::polygon_signed_area(&poly);
                        if area - me.intersection_area(&contour.tris) > OVERLAP_MIN_MM2 {
                            continue;
                        }
                        let pose = AutoPose {
                            piece: v.piece,
                            size: v.size,
                            x_mm: px,
                            y_mm: py,
                            rotation_deg: deg,
                            mirrored: v.mirrored,
                        };
                        if best.as_ref().is_none_or(|b| (by, bx) < (b.0, b.1)) {
                            best = Some((by, bx, pose, poly));
                        }
                        break 'search;
                    }
                }
            }
            match best {
                Some((_, _, pose, poly)) => {
                    poses[inst] = Some(pose);
                    placed.push(obstacle(poly));
                }
                None => {
                    if !stuck.iter().any(|s| (s.piece, s.size) == (v.piece, v.size)) {
                        stuck.push(Unplaceable {
                            piece: v.piece,
                            size: v.size,
                            reason: UnplaceableReason::NoRoomOnHide,
                        });
                    }
                }
            }
        }
        if !stuck.is_empty() {
            return Err(AutoError::Unplaceable(stuck));
        }
        debug_assert!(placed.len() == n_fixed + inst_var.len());
        let poses: Vec<AutoPose> = poses.into_iter().flatten().collect();
        let command = self.auto_command(plan, &poses, keep)?;
        let fig = self
            .verify_hide(plan, &command, &expected, &kept, min_grade)
            .map_err(AutoError::VerificationFailed)?;
        let current = self.analyze_cutting_plan(plan).ok().and_then(|a| {
            if plan_ref.placements.is_empty() {
                return None;
            }
            let y = hide_yield(Some(hide), &a)?;
            let length = a.outlines.iter().map(|o| o.bbox.max.y).fold(0.0, f64::max);
            Some(LayoutFigures {
                length_mm: length,
                yield_percent: y.percent,
            })
        });
        Ok(AutoProposal {
            command,
            poses,
            level1: fig,
            proposed: fig,
            current,
            refined: false,
            time_limited: false,
            trials: 1,
        })
    }

    /// Relecture d'une proposition sur peau : diagnostics complets (contour, défauts, écart,
    /// chevauchement), exigence de qualité, existant intact et quantités exactes.
    fn verify_hide(
        &self,
        plan: CuttingPlanId,
        command: &Command,
        expected: &HashMap<(PieceId, Option<SizeId>, bool), usize>,
        kept: &[CuttingPlacement],
        min_grade: Option<u8>,
    ) -> Result<LayoutFigures, String> {
        let mut copy = self.clone();
        copy.apply(command.clone()).map_err(|e| e.to_string())?;
        let an = copy.analyze_cutting_plan(plan).map_err(|e| e.to_string())?;
        if !an.is_valid() {
            return Err("chevauchement, contour, défaut, écart ou rotation refusés".into());
        }
        if let Some(g) = min_grade {
            if !quality_violations(&an, g).is_empty() {
                return Err("exigence de qualité non respectée".into());
            }
        }
        let plan_ref = copy.cutting_plan(plan).map_err(|e| e.to_string())?;
        if !kept.is_empty() && plan_ref.placements.get(..kept.len()) != Some(kept) {
            return Err("un placement existant a été modifié".into());
        }
        let mut got: HashMap<(PieceId, Option<SizeId>, bool), usize> = HashMap::new();
        for p in &plan_ref.placements {
            *got.entry((p.piece, p.size, p.mirrored)).or_insert(0) += 1;
        }
        if &got != expected {
            return Err("quantités posées différentes des quantités demandées".into());
        }
        let y = hide_yield(plan_ref.hide.as_ref(), &an).ok_or("métrage non calculable")?;
        let length = an
            .outlines
            .iter()
            .map(|o| o.bbox.max.y)
            .fold(0.0_f64, f64::max);
        Ok(LayoutFigures {
            length_mm: length,
            yield_percent: y.percent,
        })
    }
}
