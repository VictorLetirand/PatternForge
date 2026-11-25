//! Durcissement 1 (A2) : la résolution des points, devenue itérative et mise en cache, doit
//! donner **au bit près** les mêmes positions et les mêmes erreurs que l'ancien calcul récursif.
//!
//! L'oracle (`durcissement_oracle`) est la transcription de cet ancien calcul (récursion sans
//! cache) ; il ne partage aucun code avec la résolution testée. La campagne a été exécutée
//! d'abord sur le code d'avant la correction, où elle passait.

mod durcissement_oracle;

use durcissement_oracle::*;
use pf_core::*;
use pf_geometry::PointMm;
use std::path::{Path, PathBuf};

// ------------------------------------------------------- comparaison au bit près

fn bits(r: &R<PointMm>) -> String {
    match r {
        Ok(p) => format!("{:016x}:{:016x}", p.x.to_bits(), p.y.to_bits()),
        Err(e) => format!("{e:?}"),
    }
}

/// `Debug` d'un `f64` est la plus courte écriture qui redonne les mêmes bits.
fn dbg<T: std::fmt::Debug>(v: &T) -> String {
    format!("{v:?}")
}

fn oracle_point(s: &Sheet, id: PointId) -> String {
    bits(&o_pos(s, id, &mut Vec::new()))
}

/// Compare chaque point, chaque tracé et `evaluate` de la feuille à l'oracle.
fn compare_sheet(s: &Sheet, what: &str) {
    for p in &s.points {
        assert_eq!(
            bits(&s.position(p.id)),
            oracle_point(s, p.id),
            "{what} : position du point {}",
            p.id.0
        );
        assert_eq!(
            bits(&s.evaluate(&p.definition)),
            bits(&o_eval(s, &p.definition, "nouveau", &mut Vec::new())),
            "{what} : evaluate({})",
            p.id.0
        );
    }
    for e in &s.entities {
        assert_eq!(
            dbg(&s.shape(e.id)),
            dbg(&o_shape(s, e.id, &mut Vec::new())),
            "{what} : tracé {}",
            e.id.0
        );
        if matches!(e.kind, EntityKind::Curve { .. }) {
            assert_eq!(
                dbg(&s.curve_geometry(e.id)),
                dbg(&o_curve(s, e.id, &mut Vec::new())),
                "{what} : courbe {}",
                e.id.0
            );
        }
        if matches!(e.kind, EntityKind::Spline { .. }) {
            assert_eq!(
                dbg(&s.spline_nodes(e.id)),
                dbg(&o_spline_nodes(s, e.id, &mut Vec::new())),
                "{what} : nœuds {}",
                e.id.0
            );
        }
    }
}

// ------------------------------------------------------------ projets aléatoires

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
    /// Valeur entre `a` et `b`.
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
    }
}

const BOGUS: u64 = 999_999;

struct Gen {
    sheet: Sheet,
    next: u64,
    guides: Vec<GuideId>,
}

impl Gen {
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn point(&mut self, definition: PointDefinition) -> PointId {
        let id = PointId(self.id());
        self.sheet.points.push(Point {
            id,
            label: format!("P{}", id.0),
            definition,
            role: PointRole::Construction,
            layer: None,
        });
        id
    }

    fn entity(&mut self, kind: EntityKind) -> EntityId {
        let id = EntityId(self.id());
        self.sheet.entities.push(Entity {
            id,
            role: PathRole::Construction,
            kind,
            layer: None,
            name: None,
        });
        id
    }

    fn pick_point(&self, r: &mut Rng) -> PointId {
        if r.chance(2) {
            return PointId(BOGUS);
        }
        self.sheet.points[r.below(self.sheet.points.len())].id
    }

    fn pick_entity(&self, r: &mut Rng, wanted: fn(&EntityKind) -> bool) -> EntityId {
        let all: Vec<EntityId> = self.sheet.entities.iter().map(|e| e.id).collect();
        let good: Vec<EntityId> = self
            .sheet
            .entities
            .iter()
            .filter(|e| wanted(&e.kind))
            .map(|e| e.id)
            .collect();
        if r.chance(2) || all.is_empty() {
            EntityId(BOGUS)
        } else if !good.is_empty() && !r.chance(6) {
            good[r.below(good.len())]
        } else {
            all[r.below(all.len())]
        }
    }

    fn aim(&self, r: &mut Rng) -> AimMm {
        AimMm {
            x: r.range(-200.0, 200.0),
            y: r.range(-200.0, 200.0),
        }
    }

    fn random_definition(&mut self, r: &mut Rng) -> PointDefinition {
        use PointDefinition as D;
        let seg: fn(&EntityKind) -> bool = |k| matches!(k, EntityKind::Segment { .. });
        let cur: fn(&EntityKind) -> bool = |k| matches!(k, EntityKind::Curve { .. });
        let round: fn(&EntityKind) -> bool =
            |k| matches!(k, EntityKind::Circle { .. } | EntityKind::Arc { .. });
        let any: fn(&EntityKind) -> bool = |_| true;
        let end = if r.chance(50) {
            SegmentEnd::Start
        } else {
            SegmentEnd::End
        };
        match r.below(16) {
            0 => D::Free {
                x: r.range(-200.0, 200.0),
                y: r.range(-200.0, 200.0),
            },
            1 => D::OnSegment {
                segment: self.pick_entity(r, seg),
                distance_mm: r.range(-5.0, 80.0),
                origin: end,
            },
            2 => D::OnSegmentRatio {
                segment: self.pick_entity(r, seg),
                ratio: r.range(-0.02, 1.02),
            },
            3 => D::Intersection {
                first: self.pick_entity(r, seg),
                second: self.pick_entity(r, seg),
            },
            4 => {
                let other = if r.chance(50) { seg } else { round };
                D::CircleIntersection {
                    first: self.pick_entity(r, round),
                    second: self.pick_entity(r, other),
                    near: self.aim(r),
                }
            }
            5 => D::TraceIntersection {
                first: self.pick_entity(r, any),
                second: self.pick_entity(r, any),
                near: self.aim(r),
            },
            6 => D::OnCurve {
                curve: self.pick_entity(r, cur),
                t: r.range(-0.02, 1.02),
            },
            7 => D::AlongCurve {
                curve: self.pick_entity(r, any),
                distance_mm: r.range(-2.0, 90.0),
                origin: end,
            },
            8 => D::Offset {
                from: self.pick_point(r),
                dx_mm: r.range(-40.0, 40.0),
                dy_mm: r.range(-40.0, 40.0),
            },
            9 => D::Midpoint {
                first: self.pick_point(r),
                second: self.pick_point(r),
            },
            10 => D::Projection {
                point: self.pick_point(r),
                trace: self.pick_entity(r, any),
            },
            11 => D::Bisector {
                vertex: self.pick_point(r),
                first: self.pick_point(r),
                second: self.pick_point(r),
                distance_mm: r.range(1.0, 50.0),
            },
            12 => D::Polar {
                from: self.pick_point(r),
                distance_mm: r.range(0.5, 60.0),
                angle_deg: r.range(-360.0, 360.0),
                reference: r.chance(35).then(|| self.pick_entity(r, seg)),
            },
            13 => D::Compass {
                center_a: self.pick_point(r),
                center_b: self.pick_point(r),
                radius_a_mm: r.range(20.0, 200.0),
                radius_b_mm: r.range(20.0, 200.0),
                near: self.aim(r),
            },
            14 => D::Division {
                trace: self.pick_entity(r, any),
                ratio: r.range(0.0, 1.0),
                start: r.chance(15).then(|| self.pick_point(r)),
            },
            _ => D::Mirror {
                source: self.pick_point(r),
                axis: if r.chance(50) || self.guides.is_empty() {
                    MirrorAxis::Segment {
                        segment: self.pick_entity(r, seg),
                    }
                } else {
                    MirrorAxis::Guide {
                        guide: self.guides[r.below(self.guides.len())],
                    }
                },
            },
        }
    }

    fn random_entity(&mut self, r: &mut Rng) {
        match r.below(8) {
            0..=2 => {
                let (from, to) = (self.pick_point(r), self.pick_point(r));
                self.entity(EntityKind::Segment {
                    from,
                    to,
                    construction: None,
                });
            }
            3 => {
                let (start, end) = (self.pick_point(r), self.pick_point(r));
                let mut h = || OffsetMm {
                    dx: r.range(-30.0, 30.0),
                    dy: r.range(-30.0, 30.0),
                };
                let (handle_start, handle_end) = (h(), h());
                self.entity(EntityKind::Curve {
                    start,
                    handle_start,
                    handle_end,
                    end,
                });
            }
            4 => {
                let center = self.pick_point(r);
                self.entity(EntityKind::Circle {
                    center,
                    radius_mm: r.range(5.0, 80.0),
                    start_deg: r.range(0.0, 360.0),
                    ccw: r.chance(50),
                });
            }
            5 => {
                // Arc valide : deux points au même rayon du centre.
                let center = self.pick_point(r);
                let radius = r.range(5.0, 60.0);
                let at = |g: &mut Gen, a: f64| {
                    g.point(PointDefinition::Polar {
                        from: center,
                        distance_mm: radius,
                        angle_deg: a,
                        reference: None,
                    })
                };
                let a = r.range(0.0, 170.0);
                let start = at(self, a);
                let end = at(self, a + r.range(20.0, 150.0));
                self.entity(EntityKind::Arc {
                    center,
                    start,
                    end,
                    ccw: r.chance(50),
                });
            }
            _ => {
                let n = 2 + r.below(4);
                let mut nodes: Vec<SplineNode> = Vec::new();
                for i in 0..n {
                    let mut node = SplineNode::new(self.pick_point(r));
                    node.corner = i > 0 && i + 1 < n && r.chance(30);
                    if r.chance(25) {
                        node.tangent = Some(OffsetMm {
                            dx: r.range(-30.0, 30.0),
                            dy: r.range(-30.0, 30.0),
                        });
                    }
                    nodes.push(node);
                }
                self.entity(EntityKind::Spline { nodes });
            }
        }
    }
}

/// Feuille aléatoire : toutes les définitions, des références mortes et des cycles.
fn random_sheet(seed: u64) -> Sheet {
    let mut r = Rng(seed);
    let mut g = Gen {
        sheet: Sheet::new(SheetId(1), "Aléatoire"),
        next: 1,
        guides: Vec::new(),
    };
    for _ in 0..2 {
        let id = GuideId(g.id());
        g.sheet.guides.push(Guide {
            id,
            x_mm: r.range(-50.0, 50.0),
            y_mm: r.range(-50.0, 50.0),
            angle_deg: r.range(0.0, 180.0),
            axis: true,
            layer: None,
        });
        g.guides.push(id);
    }
    for _ in 0..(3 + r.below(5)) {
        let (x, y) = (r.range(-100.0, 100.0), r.range(-100.0, 100.0));
        g.point(PointDefinition::Free { x, y });
    }
    for _ in 0..4 {
        g.random_entity(&mut r);
    }
    for _ in 0..(6 + r.below(22)) {
        if r.chance(32) {
            g.random_entity(&mut r);
        } else {
            let d = g.random_definition(&mut r);
            g.point(d);
        }
    }
    // Une feuille sur trois : quelques références vers « plus loin » (cycles possibles).
    if r.chance(33) {
        for _ in 0..1 + r.below(2) {
            let i = r.below(g.sheet.points.len());
            let d = g.random_definition(&mut r);
            g.sheet.points[i].definition = d;
        }
    }
    g.sheet
}

/// Sous **cache partagé**, points et tracés lus dans un ordre mélangé : un résultat retenu
/// pour un point ne doit jamais changer ce que rend un autre (valeur ou refus).
fn compare_cached(s: &Sheet, seed: u64) {
    let mut r = Rng(seed ^ 0xCACE);
    let mut order: Vec<usize> = (0..s.points.len() + s.entities.len()).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, r.below(i + 1));
    }
    let expected: Vec<String> = s
        .points
        .iter()
        .map(|p| oracle_point(s, p.id))
        .chain(
            s.entities
                .iter()
                .map(|e| dbg(&o_shape(s, e.id, &mut Vec::new()))),
        )
        .collect();
    s.with_read_cache(|| {
        // Deux passes : la seconde lit un cache rempli.
        for pass in 0..2 {
            for &i in &order {
                let got = match s.points.get(i) {
                    Some(p) => bits(&s.position(p.id)),
                    None => dbg(&s.shape(s.entities[i - s.points.len()].id)),
                };
                assert_eq!(got, expected[i], "graine {seed}, passe {pass}, rang {i}");
            }
        }
    });
}

/// La validation (qui partage un cache) nomme la même première erreur de point qu'avant.
fn compare_validation(s: &Sheet, seed: u64) -> bool {
    let first = s
        .points
        .iter()
        .find_map(|p| o_pos(s, p.id, &mut Vec::new()).err());
    let mut project = Project::new("Aléatoire");
    project.sheets = vec![s.clone()];
    project.next_id = 1_000_000;
    match first {
        Some(e) => {
            assert_eq!(project.validate(), Err(e), "graine {seed} : validation");
            true
        }
        None => false,
    }
}

#[test]
fn campagne_aleatoire_positions_identiques_au_bit_pres() {
    let (mut ok, mut cycles, mut dead, mut other, mut kinds) = (0, 0, 0, 0, [0usize; 16]);
    let mut refused = 0;
    // Graine 101 écartée : elle intersecte deux courbes superposées, calcul de plusieurs secondes
    // par lecture en debug (coût de `pf_geometry::intersect`, identique avant et après).
    for seed in (0..600u64).filter(|&s| s != 101) {
        let s = random_sheet(0xD0_0000 + seed);
        compare_sheet(&s, &format!("graine {seed}"));
        compare_cached(&s, seed);
        refused += usize::from(compare_validation(&s, seed));
        for p in &s.points {
            match o_pos(&s, p.id, &mut Vec::new()) {
                Ok(_) => {
                    ok += 1;
                    kinds[kind_index(&p.definition)] += 1;
                }
                Err(CoreError::CyclicReference(_)) => cycles += 1,
                Err(CoreError::DeadReference(_)) => dead += 1,
                Err(_) => other += 1,
            }
        }
    }
    // La campagne n'est pas vide de sens : chaque sorte de point réussit parfois, et les
    // cycles, références mortes et géométries impossibles sont bien rencontrés.
    assert!(ok > 2500, "{ok}");
    assert!(
        cycles > 25 && dead > 100 && other > 100,
        "{cycles} {dead} {other}"
    );
    assert!(kinds.iter().all(|&n| n > 5), "{kinds:?}");
    assert!(refused > 60, "{refused}");
}

fn kind_index(d: &PointDefinition) -> usize {
    use PointDefinition as D;
    match d {
        D::Free { .. } => 0,
        D::OnSegment { .. } => 1,
        D::OnSegmentRatio { .. } => 2,
        D::Intersection { .. } => 3,
        D::CircleIntersection { .. } => 4,
        D::TraceIntersection { .. } => 5,
        D::OnCurve { .. } => 6,
        D::AlongCurve { .. } => 7,
        D::Offset { .. } => 8,
        D::Midpoint { .. } => 9,
        D::Projection { .. } => 10,
        D::Bisector { .. } => 11,
        D::Polar { .. } => 12,
        D::Compass { .. } => 13,
        D::Division { .. } => 14,
        D::Mirror { .. } => 15,
    }
}

fn project_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in ["examples", "crates/pf-core/tests/jeu_essai"] {
        for f in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = f.unwrap().path();
            if path.to_string_lossy().ends_with(".pforge.json") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn exemples_et_jeu_d_essai_identiques_au_bit_pres() {
    let files = project_files();
    assert!(files.len() >= 7, "{files:?}");
    let mut points = 0;
    for f in files {
        let p = persist::load(&f).unwrap();
        for s in &p.sheets {
            compare_sheet(s, &f.display().to_string());
            points += s.points.len();
        }
        for size in p.sizes.iter().flat_map(|r| &r.sizes) {
            for s in &p.graded_project(Some(size.id)).unwrap().sheets {
                compare_sheet(s, &format!("{} (taille {})", f.display(), size.label));
            }
        }
    }
    assert!(points > 50, "{points}");
}
