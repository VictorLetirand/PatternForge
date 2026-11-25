//! US-06.1b — campagne adverse des crans **en coin** : 160 séquences à graine fixe. Polygones
//! étoilés à 3 à 8 sommets (convexes quand les rayons sont égaux, sinon avec des sommets saillants
//! et rentrants), rectangles à sommet lisse (alignement exact), parcourus dans le sens horaire ou
//! anti-horaire (le contour de la pièce est alors réellement horaire : `SetPieceOutline`) ; crans en
//! coin de toutes formes, simples ou doubles, posés au début ou à la fin d'un tracé, côtés
//! changés, sommets déplacés, annuler / rétablir, aller-retour JSON, commandes refusées.
//!
//! Oracle **indépendant** : un automate tient les sommets et les crans attendus ; la bissectrice est
//! recalculée par la **somme des vecteurs unitaires** du sommet vers ses deux voisins (le code testé
//! additionne des normales de tangentes), l'intérieur / extérieur se décide par le signe de l'aire
//! (formule du lacet) et par le signe du produit vectoriel de l'angle de parcours, les traits de
//! chaque forme sont recalculés ici. Aucune fonction de `notch_corner`, `notch_shape_polylines` ni
//! de `piece_chain` n'est utilisée pour attendre une valeur. Propriétés :
//!
//! 1. jamais de panique, résultat identique à graine égale ;
//! 2. sommet, direction (intérieure / extérieure selon le côté du cran), angle intérieur et traits
//!    de chaque forme conformes à l'oracle après **chaque** commande, sommets déplacés compris ;
//! 3. aucun contrôle `CornerNotchesFailed` sur ces contours valides ;
//! 4. refus (cran hors sommet, « sur la coupe » sur un coin) : projet identique à l'octet ; no-op
//!    sans étape ; décocher puis recocher rend le projet identique à l'octet ;
//! 5. annuler : exactement une étape par changement réel, jusqu'à l'état de départ ;
//! 6. aller-retour JSON exact (valeur et octets).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use std::f64::consts::TAU;
use std::panic::{catch_unwind, AssertUnwindSafe};

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
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// Arrondi au centième de mm.
fn q(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

type V2 = (f64, f64);

fn sub(a: V2, b: V2) -> V2 {
    (a.0 - b.0, a.1 - b.1)
}
fn add(a: V2, b: V2) -> V2 {
    (a.0 + b.0, a.1 + b.1)
}
fn mul(a: V2, k: f64) -> V2 {
    (a.0 * k, a.1 * k)
}
fn dot(a: V2, b: V2) -> f64 {
    a.0 * b.0 + a.1 * b.1
}
fn cross(a: V2, b: V2) -> f64 {
    a.0 * b.1 - a.1 * b.0
}
fn norm(a: V2) -> f64 {
    a.0.hypot(a.1)
}
fn unit(a: V2) -> V2 {
    mul(a, 1.0 / norm(a))
}
fn left(a: V2) -> V2 {
    (-a.1, a.0)
}

#[derive(Clone, Debug)]
struct ON {
    id: NotchId,
    edge: usize,
    at_end: bool,
    side: Side,
    depth: f64,
    double: bool,
    form: NotchForm,
    width: Option<f64>,
}

struct Oracle {
    /// Sommets dans l'ordre des tracés (`edges[i]` : `v[i]` → `v[i+1]`).
    v: Vec<V2>,
    /// Angle polaire de chaque sommet (pour les déplacements radiaux) ; vide = sommets figés.
    theta: Vec<f64>,
    radius_max: f64,
    notches: Vec<ON>,
}

#[derive(Default, Debug, PartialEq)]
struct Counters {
    convex: usize,
    reflex: usize,
    smooth: usize,
    clockwise: usize,
    counter_clockwise: usize,
    forms: [usize; 5],
    doubles: usize,
    inward: usize,
    outward: usize,
    ends: usize,
    starts: usize,
    moves: usize,
    refusals: usize,
    noops: usize,
    toggles: usize,
    undone: usize,
    roundtrips: usize,
    marks_checked: usize,
}

impl Oracle {
    fn n(&self) -> usize {
        self.v.len()
    }
    fn area2(&self) -> f64 {
        let n = self.n();
        (0..n).map(|i| cross(self.v[i], self.v[(i + 1) % n])).sum()
    }
    fn edge_len(&self, i: usize) -> f64 {
        norm(sub(self.v[(i + 1) % self.n()], self.v[i]))
    }
    fn vertex_of(&self, n: &ON) -> usize {
        if n.at_end {
            (n.edge + 1) % self.n()
        } else {
            n.edge
        }
    }

    /// Direction attendue (unitaire), angle intérieur (degrés) et nature du sommet
    /// (`'c'` saillant, `'r'` rentrant, `'s'` lisse).
    fn expect(&self, n: &ON) -> (V2, f64, char) {
        let cnt = self.n();
        let k = self.vertex_of(n);
        let (prev, vk, next) = (
            self.v[(k + cnt - 1) % cnt],
            self.v[k],
            self.v[(k + 1) % cnt],
        );
        let area = self.area2();
        let u1 = unit(sub(prev, vk));
        let u2 = unit(sub(next, vk));
        let bis = add(u1, u2);
        let (inward, angle, kind) = if norm(bis) < 1e-9 {
            // Sommet lisse : l'intérieur est à gauche du parcours si l'aire est positive.
            let t = unit(sub(next, vk));
            let inw = if area > 0.0 {
                left(t)
            } else {
                mul(left(t), -1.0)
            };
            (inw, 180.0, 's')
        } else {
            // Saillant si l'angle de parcours tourne dans le sens de l'aire.
            let convex = cross(sub(vk, prev), sub(next, vk)) * area > 0.0;
            let a = dot(u1, u2).clamp(-1.0, 1.0).acos().to_degrees();
            if convex {
                (unit(bis), a, 'c')
            } else {
                (mul(unit(bis), -1.0), 360.0 - a, 'r')
            }
        };
        let t_edge = unit(sub(self.v[(n.edge + 1) % cnt], self.v[n.edge]));
        let side_normal = mul(left(t_edge), if n.side == Side::Left { 1.0 } else { -1.0 });
        let out_edge = if area > 0.0 {
            mul(left(t_edge), -1.0)
        } else {
            left(t_edge)
        };
        let dir = if dot(side_normal, out_edge) > 0.0 {
            mul(inward, -1.0)
        } else {
            inward
        };
        (dir, angle, kind)
    }

    /// Traits attendus d'un cran : forme posée au sommet, vers `dir`.
    fn marks(&self, n: &ON) -> Vec<(V2, V2)> {
        let (d, _, _) = self.expect(n);
        let p = left(d);
        let vertex = self.v[self.vertex_of(n)];
        let feet: Vec<V2> = if n.double {
            vec![add(vertex, mul(p, 1.5)), add(vertex, mul(p, -1.5))]
        } else {
            vec![vertex]
        };
        let w = n.width.unwrap_or(3.0);
        let h = w / 2.0;
        let mut out = Vec::new();
        for f in feet {
            let at = |a: f64, b: f64| add(f, add(mul(p, a), mul(d, b)));
            match n.form {
                NotchForm::Line => out.push((f, at(0.0, n.depth))),
                NotchForm::T => {
                    out.push((f, at(0.0, n.depth)));
                    out.push((at(-h, n.depth), at(h, n.depth)));
                }
                NotchForm::V => {
                    out.push((at(-h, 0.0), at(0.0, n.depth)));
                    out.push((at(0.0, n.depth), at(h, 0.0)));
                }
                NotchForm::Slot => {
                    out.push((at(-h, 0.0), at(-h, n.depth)));
                    out.push((at(-h, n.depth), at(h, n.depth)));
                    out.push((at(h, n.depth), at(h, 0.0)));
                }
                NotchForm::U => {
                    let mut pts = vec![at(-h, 0.0)];
                    for k in 0..=12 {
                        let th = std::f64::consts::PI * (1.0 - k as f64 / 12.0);
                        pts.push(at(h * th.cos(), n.depth - h + h * th.sin()));
                    }
                    pts.push(at(h, 0.0));
                    out.extend(pts.windows(2).map(|w| (w[0], w[1])));
                }
            }
        }
        out
    }
}

fn pt(p: V2) -> PointMm {
    PointMm::new(p.0, p.1)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn seg_match(a: (PointMm, PointMm), b: (V2, V2)) -> bool {
    let c = |x: PointMm, y: V2| (x.x - y.0).abs() < 1e-6 && (x.y - y.1).abs() < 1e-6;
    (c(a.0, b.0) && c(a.1, b.1)) || (c(a.0, b.1) && c(a.1, b.0))
}

/// Le polygone en `Project` : feuille, tracés, pièce (contour horaire si `cw`).
fn build(v: &[V2], cw: bool) -> (Project, SheetId, PieceId, Vec<EntityId>, Vec<PointId>) {
    let mut p = Project::new("Adverse coins");
    let s = p.sheets[0].id;
    let n = v.len();
    let edges: Vec<EntityId> = (0..n)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(v[i])),
                to: PointRef::New(pt(v[(i + 1) % n])),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    if cw {
        p.apply(Command::SetPieceOutline {
            piece,
            outline: edges
                .iter()
                .map(|&entity| PieceEdge {
                    entity,
                    reversed: false,
                })
                .collect(),
            grainline: None,
        })
        .unwrap();
        assert!(p.piece_contour(piece).unwrap().clockwise);
    } else {
        assert!(!p.piece_contour(piece).unwrap().clockwise);
    }
    let sh = p.sheet(s).unwrap();
    let pids = v
        .iter()
        .map(|c| {
            sh.points
                .iter()
                .find(|q| {
                    let x = sh.position(q.id).unwrap();
                    (x.x - c.0).abs() < 1e-9 && (x.y - c.1).abs() < 1e-9
                })
                .expect("point du sommet")
                .id
        })
        .collect();
    (p, s, piece, edges, pids)
}

const FORMS: [NotchForm; 5] = [
    NotchForm::Line,
    NotchForm::T,
    NotchForm::V,
    NotchForm::U,
    NotchForm::Slot,
];

fn form_index(f: NotchForm) -> usize {
    FORMS.iter().position(|x| *x == f).unwrap()
}

/// Contrôle complet du projet contre l'oracle, après une commande.
fn check(doc: &Document, s: SheetId, piece: PieceId, o: &Oracle, c: &mut Counters, tag: &str) {
    let p = doc.project();
    for n in &o.notches {
        let info = p
            .notch_corner(s, n.id)
            .unwrap_or_else(|e| panic!("{tag} : sommet illisible : {e}"));
        let (dir, angle, kind) = o.expect(n);
        let vertex = o.v[o.vertex_of(n)];
        assert!(
            (info.vertex.x - vertex.0).abs() < 1e-6 && (info.vertex.y - vertex.1).abs() < 1e-6,
            "{tag} : sommet {:?}, attendu {vertex:?}",
            info.vertex
        );
        assert!(
            (info.direction.x - dir.0).abs() < 1e-9 && (info.direction.y - dir.1).abs() < 1e-9,
            "{tag} : direction {:?}, attendue {dir:?} (sommet {kind})",
            info.direction
        );
        assert!(
            (info.interior_angle_deg - angle).abs() < 1e-5,
            "{tag} : angle {}, attendu {angle}",
            info.interior_angle_deg
        );
        // Traits : même ensemble de segments (ordre et sens indifférents).
        let notch = p.sheet(s).unwrap().notch(n.id).unwrap();
        assert!(notch.corner, "{tag} : drapeau perdu");
        let got = p.notch_placed_marks(s, notch).unwrap().1;
        let mut want = o.marks(n);
        assert_eq!(got.len(), want.len(), "{tag} : nombre de traits");
        for g in &got {
            let i = want
                .iter()
                .position(|w| seg_match(*g, *w))
                .unwrap_or_else(|| panic!("{tag} : trait {g:?} absent de l'oracle {want:?}"));
            want.swap_remove(i);
            c.marks_checked += 1;
        }
        match kind {
            'c' => c.convex += 1,
            'r' => c.reflex += 1,
            _ => c.smooth += 1,
        }
        c.forms[form_index(n.form)] += 1;
        if n.double {
            c.doubles += 1;
        }
        if n.at_end {
            c.ends += 1;
        } else {
            c.starts += 1;
        }
        // Direction du trait : s'éloigne du centre du polygone, ou s'en rapproche.
        if dot(dir, unit(sub(vertex, centroid(&o.v)))) >= 0.0 {
            c.outward += 1;
        } else {
            c.inward += 1;
        }
    }
    assert!(
        !p.piece_issues(piece)
            .iter()
            .any(|i| matches!(i, PieceIssue::CornerNotchesFailed { .. })),
        "{tag} : contrôle CornerNotchesFailed inattendu"
    );
}

fn centroid(v: &[V2]) -> V2 {
    let k = v.len() as f64;
    (
        v.iter().map(|p| p.0).sum::<f64>() / k,
        v.iter().map(|p| p.1).sum::<f64>() / k,
    )
}

/// Une séquence : retourne la signature finale (octets) pour la comparaison à graine égale.
fn sequence(seed: u64, c: &mut Counters) -> String {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0xC0FFEE);
    let cw = rng.chance(50);
    let smooth = rng.chance(20);
    let (mut verts, theta, radius_max): (Vec<V2>, Vec<f64>, f64) = if smooth {
        // Rectangle à coordonnées entières paires ; sommet lisse au milieu du bord bas.
        let w = 2.0 * (10 + rng.below(40)) as f64;
        let h = 2.0 * (8 + rng.below(30)) as f64;
        (
            vec![(0.0, 0.0), (w / 2.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
            Vec::new(),
            0.0,
        )
    } else {
        let n = 3 + rng.below(6);
        let big = rng.range(40.0, 120.0);
        let regular = rng.chance(30);
        // Rayons alternés : une pointe sur deux est un sommet rentrant.
        let zigzag = rng.chance(50);
        let phase = rng.range(0.0, TAU);
        let mut v = Vec::new();
        let mut th = Vec::new();
        for k in 0..n {
            let a = phase + TAU * k as f64 / n as f64;
            let r = if regular {
                big
            } else if zigzag {
                big * if k % 2 == 0 {
                    rng.range(0.8, 1.0)
                } else {
                    rng.range(0.3, 0.6)
                }
            } else {
                rng.range(0.3, 1.0) * big
            };
            v.push((q(r * a.cos()), q(r * a.sin())));
            th.push(a);
        }
        (v, th, big)
    };
    let mut theta = theta;
    if cw {
        verts.reverse();
        theta.reverse();
        c.clockwise += 1;
    } else {
        c.counter_clockwise += 1;
    }
    let (p, s, piece, edges, pids) = build(&verts, cw);
    let start = bytes(&p);
    let mut doc = Document::new(p);
    let mut o = Oracle {
        v: verts,
        theta,
        radius_max,
        notches: Vec::new(),
    };
    // Sens du polygone confirmé par l'aire de l'oracle.
    assert_eq!(o.area2() < 0.0, cw, "graine {seed} : sens du polygone");
    let mut stack: Vec<String> = Vec::new();
    let tag = |i: usize| format!("graine {seed}, op {i}");

    // Applique une commande qui doit réussir : une étape si le projet change, aucune sinon.
    let run = |doc: &mut Document,
               stack: &mut Vec<String>,
               cmd: Command,
               expect_change: bool,
               c: &mut Counters,
               tag: &str|
     -> Outcome {
        let before = bytes(doc.project());
        let out = doc
            .execute(cmd)
            .unwrap_or_else(|e| panic!("{tag} : commande refusée : {e}"));
        let after = bytes(doc.project());
        if before == after {
            assert!(
                !expect_change,
                "{tag} : aucun changement alors qu'il en faut un"
            );
            c.noops += 1;
        } else {
            stack.push(before);
        }
        out
    };

    let ops = 6 + rng.below(9);
    for i in 0..ops {
        let t = tag(i);
        let roll = rng.below(100);
        let kind_add = roll < 40 || o.notches.is_empty();
        if kind_add {
            let cnt = o.n();
            let k = rng.below(cnt);
            // Le sommet k est le début de edges[k] ou la fin de edges[k-1].
            let (edge, at_end) = if rng.chance(50) {
                (k, false)
            } else {
                ((k + cnt - 1) % cnt, true)
            };
            let side = if rng.chance(50) {
                Side::Left
            } else {
                Side::Right
            };
            let depth = q(rng.range(3.0, 12.0));
            let at = if at_end { o.edge_len(edge) } else { 0.0 };
            let id = run(
                &mut doc,
                &mut stack,
                Command::AddNotch {
                    sheet: s,
                    entity: edges[edge],
                    distance_mm: at,
                    side,
                    length_mm: depth,
                    style: NotchStyle::Single,
                },
                true,
                c,
                &t,
            )
            .notches[0];
            run(
                &mut doc,
                &mut stack,
                Command::SetNotchCorner {
                    sheet: s,
                    notch: id,
                    corner: true,
                },
                true,
                c,
                &t,
            );
            o.notches.push(ON {
                id,
                edge,
                at_end,
                side,
                depth,
                double: false,
                form: NotchForm::Line,
                width: None,
            });
        } else if roll < 52 {
            // Changer de côté : l'intérieur et l'extérieur s'échangent.
            let j = rng.below(o.notches.len());
            let n = o.notches[j].clone();
            let at = if n.at_end { o.edge_len(n.edge) } else { 0.0 };
            let side = if n.side == Side::Left {
                Side::Right
            } else {
                Side::Left
            };
            run(
                &mut doc,
                &mut stack,
                Command::SetNotch {
                    sheet: s,
                    notch: n.id,
                    distance_mm: at,
                    side,
                    length_mm: n.depth,
                    style: if n.double {
                        NotchStyle::Double
                    } else {
                        NotchStyle::Single
                    },
                },
                true,
                c,
                &t,
            );
            o.notches[j].side = side;
        } else if roll < 65 {
            let j = rng.below(o.notches.len());
            let form = FORMS[rng.below(5)];
            let width = (!form.is_line()).then(|| q(rng.range(1.0, 6.0)));
            let n = &o.notches[j];
            let unchanged = n.form == form && n.width == width;
            run(
                &mut doc,
                &mut stack,
                Command::SetNotchShape {
                    sheet: s,
                    notch: n.id,
                    form,
                    width_mm: width,
                    on_cut: false,
                },
                !unchanged,
                c,
                &t,
            );
            o.notches[j].form = form;
            o.notches[j].width = width;
        } else if roll < 72 {
            let j = rng.below(o.notches.len());
            let n = o.notches[j].clone();
            let at = if n.at_end { o.edge_len(n.edge) } else { 0.0 };
            run(
                &mut doc,
                &mut stack,
                Command::SetNotch {
                    sheet: s,
                    notch: n.id,
                    distance_mm: at,
                    side: n.side,
                    length_mm: n.depth,
                    style: if n.double {
                        NotchStyle::Single
                    } else {
                        NotchStyle::Double
                    },
                },
                true,
                c,
                &t,
            );
            o.notches[j].double = !n.double;
        } else if roll < 83 && !o.theta.is_empty() {
            // Déplacement radial d'un sommet : le polygone reste étoilé, donc simple.
            let k = rng.below(o.n());
            let r = q(rng.range(0.3, 1.0) * o.radius_max).max(5.0);
            let to = (q(r * o.theta[k].cos()), q(r * o.theta[k].sin()));
            let changed = (to.0 - o.v[k].0).abs() > 1e-12 || (to.1 - o.v[k].1).abs() > 1e-12;
            run(
                &mut doc,
                &mut stack,
                Command::MovePoint {
                    sheet: s,
                    point: pids[k],
                    to: pt(to),
                },
                changed,
                c,
                &t,
            );
            o.v[k] = to;
            c.moves += 1;
        } else if roll < 89 {
            // Refus : cran au milieu d'un tracé, jamais en coin ; projet identique à l'octet.
            let e = rng.below(o.n());
            let at = q(o.edge_len(e) * rng.range(0.2, 0.8));
            let id = run(
                &mut doc,
                &mut stack,
                Command::AddNotch {
                    sheet: s,
                    entity: edges[e],
                    distance_mm: at,
                    side: Side::Right,
                    length_mm: 5.0,
                    style: NotchStyle::Single,
                },
                true,
                c,
                &t,
            )
            .notches[0];
            let before = bytes(doc.project());
            let err = doc.execute(Command::SetNotchCorner {
                sheet: s,
                notch: id,
                corner: true,
            });
            assert!(err.is_err(), "{t} : cran mi-tracé accepté en coin");
            assert_eq!(bytes(doc.project()), before, "{t} : refus avec trace");
            c.refusals += 1;
            run(
                &mut doc,
                &mut stack,
                Command::DeleteNotch {
                    sheet: s,
                    notch: id,
                },
                true,
                c,
                &t,
            );
        } else if roll < 93 {
            // Refus : « sur la coupe » sur un coin, et coin sur un cran « sur la coupe ».
            let j = rng.below(o.notches.len());
            let n = o.notches[j].clone();
            let before = bytes(doc.project());
            let err = doc.execute(Command::SetNotchShape {
                sheet: s,
                notch: n.id,
                form: n.form,
                width_mm: n.width,
                on_cut: true,
            });
            assert!(err.is_err(), "{t} : sur la coupe accepté sur un coin");
            assert_eq!(bytes(doc.project()), before, "{t} : refus avec trace");
            c.refusals += 1;
        } else if roll < 97 {
            // No-op : cocher un coin déjà coché.
            let j = rng.below(o.notches.len());
            let before = bytes(doc.project());
            run(
                &mut doc,
                &mut stack,
                Command::SetNotchCorner {
                    sheet: s,
                    notch: o.notches[j].id,
                    corner: true,
                },
                false,
                c,
                &t,
            );
            assert_eq!(bytes(doc.project()), before);
        } else {
            // Décocher puis recocher : projet identique à l'octet, deux étapes.
            let j = rng.below(o.notches.len());
            let id = o.notches[j].id;
            let before = bytes(doc.project());
            for corner in [false, true] {
                run(
                    &mut doc,
                    &mut stack,
                    Command::SetNotchCorner {
                        sheet: s,
                        notch: id,
                        corner,
                    },
                    true,
                    c,
                    &t,
                );
            }
            assert_eq!(bytes(doc.project()), before, "{t} : bascule non réversible");
            c.toggles += 1;
        }
        check(&doc, s, piece, &o, c, &t);

        // Annuler / rétablir une fois sur quatre : exactement une étape.
        if rng.chance(25) && !stack.is_empty() {
            let after = bytes(doc.project());
            assert!(doc.undo());
            assert_eq!(
                bytes(doc.project()),
                *stack.last().unwrap(),
                "{t} : annulation d'une étape"
            );
            assert!(doc.redo());
            assert_eq!(bytes(doc.project()), after, "{t} : rétablir");
            c.undone += 1;
        }
        // Aller-retour JSON : valeur et octets.
        if rng.chance(20) {
            let json = bytes(doc.project());
            let back = persist::from_json(&json).unwrap();
            assert_eq!(&back, doc.project(), "{t} : aller-retour (valeur)");
            assert_eq!(bytes(&back), json, "{t} : aller-retour (octets)");
            c.roundtrips += 1;
        }
    }
    let end = bytes(doc.project());
    // Tout annuler : une étape par entrée de la pile, état de départ retrouvé.
    let mut undone = 0;
    while doc.undo() {
        undone += 1;
    }
    assert_eq!(undone, stack.len(), "graine {seed} : nombre d'étapes");
    assert_eq!(
        bytes(doc.project()),
        start,
        "graine {seed} : état de départ"
    );
    end
}

fn campaign(seeds: std::ops::Range<u64>) -> (Counters, Vec<String>) {
    let mut c = Counters::default();
    let mut ends = Vec::new();
    for seed in seeds {
        let r = catch_unwind(AssertUnwindSafe(|| sequence(seed, &mut c)));
        match r {
            Ok(end) => ends.push(end),
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                panic!("séquence {seed} : {msg}");
            }
        }
    }
    (c, ends)
}

#[test]
fn campagne_adverse_crans_en_coin_160_sequences() {
    let (c, ends) = campaign(0..160);
    assert_eq!(ends.len(), 160);
    // Les contrôles forts ont bien été exécutés (jamais un test vide).
    assert!(c.convex >= 300, "{c:?}");
    assert!(c.reflex >= 150, "{c:?}");
    assert!(c.smooth >= 20, "{c:?}");
    assert!(c.clockwise >= 50 && c.counter_clockwise >= 50, "{c:?}");
    assert!(c.forms.iter().all(|&f| f >= 50), "{c:?}");
    assert!(c.doubles >= 100, "{c:?}");
    assert!(c.inward >= 100 && c.outward >= 100, "{c:?}");
    assert!(c.starts >= 100 && c.ends >= 100, "{c:?}");
    assert!(c.moves >= 100, "{c:?}");
    assert!(c.refusals >= 100, "{c:?}");
    assert!(c.noops >= 20 && c.toggles >= 10, "{c:?}");
    assert!(c.undone >= 100 && c.roundtrips >= 100, "{c:?}");
    assert!(c.marks_checked >= 2000, "{c:?}");
}

#[test]
fn campagne_adverse_crans_en_coin_est_deterministe() {
    let (c1, e1) = campaign(0..24);
    let (c2, e2) = campaign(0..24);
    assert_eq!(c1, c2);
    assert_eq!(e1, e2);
}
