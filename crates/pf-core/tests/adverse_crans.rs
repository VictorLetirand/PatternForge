//! US-06.1 — campagne adverse des crans avancés : 160 séquences à graine fixe (rectangles et
//! polygones réguliers à 3 à 8 côtés tournés au hasard ; marge de couture par défaut ou par bord,
//! coupe nette, crans de toutes formes et de tous styles posés sur la couture ou « sur la coupe »,
//! changements de forme et de profondeur, annuler / rétablir, aller-retour JSON, commandes refusées).
//!
//! Oracle **indépendant** : un petit automate (`OState`) tient l'état attendu et décide seul quelles
//! commandes doivent être refusées ; le contour de coupe est reconstruit ici comme intersection des
//! droites décalées (polygone convexe, coins en onglet, angles ≥ 60°), la projection est un tir de
//! rayon contre ce polygone, et les traits de chaque forme sont recalculés avec une rotation
//! explicite : aucune fonction de `piece_cut_contour`, `notch_on_cut` ni de `notch_shape_polylines`
//! n'est utilisée pour attendre une valeur. Propriétés :
//!
//! 1. jamais de panique, résultat identique à graine égale ;
//! 2. un cran « sur la coupe » est exactement au pied de la perpendiculaire à la couture, à la
//!    distance de la marge (jusqu'à 1e-6 mm), après chaque changement de marge ; sans marge ou en
//!    coupe nette la coupe est la couture ;
//! 3. chaque forme (trait, T, V, U, fente), simple ou double, sur la couture ou sur la coupe, donne
//!    exactement les segments de l'oracle ; un cran sur la coupe n'a aucun trait sur la couture ;
//! 4. refus (largeur sur un trait, largeur hors bornes, U trop large, sur la coupe hors pièce,
//!    marge sur coupe nette, coupe nette sur marge) : projet identique à l'octet ; tout le reste réussit ;
//! 5. annuler / rétablir : exactement une étape par changement réel, aucune pour un no-op ;
//! 6. aller-retour JSON exact (valeur et octets) ; aucun contrôle `CutContourFailed` ni
//!    `CutNotchesFailed` sur ces polygones convexes.

use pf_core::notch::NotchPlace;
use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Side};
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

// ------------------------------------------------------------------ oracle

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Plain,
    Seam,
    Net,
}

#[derive(Clone, PartialEq, Debug)]
struct ON {
    id: NotchId,
    edge: usize,
    at: f64,
    side: Side,
    depth: f64,
    double: bool,
    form: NotchForm,
    width: Option<f64>,
    on_cut: bool,
}

#[derive(Clone, PartialEq, Debug)]
struct OState {
    v: Vec<V2>,
    mode: Mode,
    default: f64,
    over: Vec<Option<f64>>,
    notches: Vec<ON>,
}

fn sub(a: V2, b: V2) -> V2 {
    (a.0 - b.0, a.1 - b.1)
}
fn len(a: V2) -> f64 {
    a.0.hypot(a.1)
}
fn unit(a: V2) -> V2 {
    let l = len(a);
    (a.0 / l, a.1 / l)
}

impl OState {
    fn n(&self) -> usize {
        self.v.len()
    }
    fn edge_len(&self, i: usize) -> f64 {
        len(sub(self.v[(i + 1) % self.n()], self.v[i]))
    }
    fn tangent(&self, i: usize) -> V2 {
        unit(sub(self.v[(i + 1) % self.n()], self.v[i]))
    }
    /// Normale extérieure (polygone anti-horaire : l'extérieur est à droite du sens de parcours).
    fn outward(&self, i: usize) -> V2 {
        let t = self.tangent(i);
        (t.1, -t.0)
    }
    fn margin(&self, i: usize) -> f64 {
        match self.mode {
            Mode::Seam => self.over[i].unwrap_or(self.default),
            _ => 0.0,
        }
    }
    /// Polygone de coupe : intersection des droites décalées de leur marge (coins en onglet).
    fn cut_polygon(&self) -> Vec<V2> {
        let n = self.n();
        (0..n)
            .map(|j| {
                let (a, b) = ((j + n - 1) % n, j);
                let (na, nb) = (self.outward(a), self.outward(b));
                let ca = self.v[a].0 * na.0 + self.v[a].1 * na.1 + self.margin(a);
                let cb = self.v[b].0 * nb.0 + self.v[b].1 * nb.1 + self.margin(b);
                let det = na.0 * nb.1 - na.1 * nb.0;
                ((ca * nb.1 - na.1 * cb) / det, (na.0 * cb - ca * nb.0) / det)
            })
            .collect()
    }
    fn edge_point(&self, i: usize, d: f64) -> V2 {
        let t = self.tangent(i);
        (self.v[i].0 + t.0 * d, self.v[i].1 + t.1 * d)
    }
}

/// Premier croisement (s ≥ 0) du rayon `p + s·dir` avec le polygone fermé `poly`.
fn cast(p: V2, dir: V2, poly: &[V2]) -> Option<f64> {
    let n = poly.len();
    let mut best: Option<f64> = None;
    for j in 0..n {
        let (a, b) = (poly[j], poly[(j + 1) % n]);
        let e = sub(b, a);
        let det = dir.0 * -e.1 - dir.1 * -e.0;
        if det.abs() < 1e-12 {
            continue;
        }
        let w = sub(a, p);
        let s = (w.0 * -e.1 - w.1 * -e.0) / det;
        let u = (dir.0 * w.1 - dir.1 * w.0) / det;
        if s >= -1e-9 && (-1e-9..=1.0 + 1e-9).contains(&u) && best.is_none_or(|b| s < b) {
            best = Some(s);
        }
    }
    best
}

/// Segments d'une forme dans le repère (`along`, `into`) : réimplémentation indépendante.
fn shape_segments(
    form: NotchForm,
    foot: V2,
    along: V2,
    into: V2,
    depth: f64,
    width: f64,
) -> Vec<(V2, V2)> {
    let at = |a: f64, b: f64| -> V2 {
        (
            foot.0 + a * along.0 + b * into.0,
            foot.1 + a * along.1 + b * into.1,
        )
    };
    let h = width / 2.0;
    let chain = |pts: Vec<V2>| -> Vec<(V2, V2)> { pts.windows(2).map(|w| (w[0], w[1])).collect() };
    match form {
        NotchForm::Line => vec![(at(0.0, 0.0), at(0.0, depth))],
        NotchForm::T => vec![
            (at(0.0, 0.0), at(0.0, depth)),
            (at(-h, depth), at(h, depth)),
        ],
        NotchForm::V => chain(vec![at(-h, 0.0), at(0.0, depth), at(h, 0.0)]),
        NotchForm::Slot => chain(vec![at(-h, 0.0), at(-h, depth), at(h, depth), at(h, 0.0)]),
        NotchForm::U => {
            let c = depth - h;
            let mut pts = vec![at(-h, 0.0)];
            for k in 0..=12 {
                let ang = 180.0 - 15.0 * k as f64;
                let (sn, cs) = ang.to_radians().sin_cos();
                pts.push(at(h * cs, c + h * sn));
            }
            pts.push(at(h, 0.0));
            chain(pts)
        }
    }
}

/// Traits attendus d'un cran : lieu (couture ou coupe) et segments.
fn expected(st: &OState, n: &ON) -> (NotchPlace, Vec<(V2, V2)>, Vec<V2>) {
    let l = st.edge_len(n.edge);
    let t = st.tangent(n.edge);
    let out = st.outward(n.edge);
    let width = n.width.unwrap_or(3.0);
    let offsets: &[f64] = if n.double { &[-1.5, 1.5] } else { &[0.0] };
    let cut_poly = st.cut_polygon();
    let mut segs = Vec::new();
    let mut feet = Vec::new();
    for o in offsets {
        let d = (n.at + o).clamp(0.0, l);
        let p = st.edge_point(n.edge, d);
        if n.on_cut {
            let s = cast(p, out, &cut_poly).expect("l'oracle projette toujours (convexe)");
            let foot = (p.0 + out.0 * s, p.1 + out.1 * s);
            feet.push(foot);
            segs.extend(shape_segments(
                n.form,
                foot,
                t,
                (-out.0, -out.1),
                n.depth,
                width,
            ));
        } else {
            let k = if n.side == Side::Left { 1.0 } else { -1.0 };
            let into = (-t.1 * k, t.0 * k);
            segs.extend(shape_segments(n.form, p, t, into, n.depth, width));
        }
    }
    let place = if n.on_cut && st.mode == Mode::Seam {
        NotchPlace::Cut
    } else {
        NotchPlace::Seam
    };
    (place, segs, feet)
}

// ------------------------------------------------------------------ scène

#[derive(Default)]
struct Counters {
    sequences: usize,
    steps: usize,
    checks: usize,
    /// Pieds de crans sur la coupe comparés à l'oracle.
    strong_feet: usize,
    /// … dont ceux d'un bord non parallèle aux axes (perpendiculaire à la couture inclinée).
    slanted_feet: usize,
    /// … avec une marge qui a changé depuis la vérification précédente du même cran.
    margin_followed: usize,
    /// Segments de forme comparés (sur la couture et sur la coupe).
    strong_segments: usize,
    forms_on_cut: [usize; 5],
    forms_on_seam: [usize; 5],
    doubles: usize,
    net_checks: usize,
    plain_cut_checks: usize,
    refused_width_on_line: usize,
    refused_width_bounds: usize,
    refused_u_too_wide: usize,
    refused_off_piece: usize,
    refused_seam_on_net: usize,
    refused_net_on_seam: usize,
    refused_depth: usize,
    undos: usize,
    redos: usize,
    noops: usize,
    roundtrips: usize,
}

fn form_index(f: NotchForm) -> usize {
    NotchForm::ALL.iter().position(|x| *x == f).unwrap()
}

struct World {
    doc: Document,
    sheet: SheetId,
    piece: PieceId,
    edges: Vec<EntityId>,
    free: EntityId,
    st: OState,
    past: Vec<(String, OState)>,
    future: Vec<(String, OState)>,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn build(rng: &mut Rng, index: usize) -> World {
    let (cx, cy) = (q(rng.range(-60.0, 160.0)), q(rng.range(-60.0, 160.0)));
    let v: Vec<V2> = if index % 4 == 0 {
        let (w, h) = (rng.range(45.0, 150.0), rng.range(45.0, 150.0));
        let rot = if rng.chance(50) {
            rng.range(0.0, std::f64::consts::TAU)
        } else {
            0.0
        };
        let base = [
            (-w / 2.0, -h / 2.0),
            (w / 2.0, -h / 2.0),
            (w / 2.0, h / 2.0),
            (-w / 2.0, h / 2.0),
        ];
        let (sn, cs) = rot.sin_cos();
        base.iter()
            .map(|(x, y)| (q(cx + x * cs - y * sn), q(cy + x * sn + y * cs)))
            .collect()
    } else {
        let n = 3 + rng.below(6);
        let r = rng.range(40.0, 120.0);
        let rot = rng.range(0.0, std::f64::consts::TAU);
        (0..n)
            .map(|k| {
                let a = rot + std::f64::consts::TAU * k as f64 / n as f64;
                (q(cx + r * a.cos()), q(cy + r * a.sin()))
            })
            .collect()
    };
    let mut p = Project::new("Adverse crans");
    let s = p.sheets[0].id;
    let n = v.len();
    let edges: Vec<EntityId> = (0..n)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(v[i].0, v[i].1)),
                to: PointRef::New(pt(v[(i + 1) % n].0, v[(i + 1) % n].1)),
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
    let free = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(500.0, 0.0)),
            to: PointRef::New(pt(560.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    World {
        doc: Document::new(p),
        sheet: s,
        piece,
        edges,
        free,
        st: OState {
            v,
            mode: Mode::Plain,
            default: 0.0,
            over: vec![None; n],
            notches: Vec::new(),
        },
        past: Vec::new(),
        future: Vec::new(),
    }
}

impl World {
    /// Exécute une commande ; une étape (changement réel) alimente l'historique de l'oracle.
    fn run(&mut self, command: Command, next: OState, c: &mut Counters) -> Result<Outcome, String> {
        let before = bytes(self.doc.project());
        match self.doc.execute(command) {
            Ok(out) => {
                let after = bytes(self.doc.project());
                if after != before {
                    self.past.push((before, self.st.clone()));
                    self.future.clear();
                } else {
                    c.noops += 1;
                }
                self.st = next;
                Ok(out)
            }
            Err(e) => {
                assert_eq!(
                    bytes(self.doc.project()),
                    before,
                    "refus sans trace (octets) : {e}"
                );
                Err(e.to_string())
            }
        }
    }

    fn seam_command(&self, st: &OState) -> Command {
        let seam = (st.mode == Mode::Seam).then(|| SeamAllowance {
            default_mm: st.default,
            edges: st
                .over
                .iter()
                .enumerate()
                .filter_map(|(i, m)| {
                    m.map(|mm| EdgeAllowance {
                        entity: self.edges[i],
                        start_mm: mm,
                        end_mm: mm,
                    })
                })
                .collect(),
            ..Default::default()
        });
        Command::SetSeamAllowance {
            piece: self.piece,
            seam,
        }
    }
}

// ------------------------------------------------------------------ contrôles

fn close(a: PointMm, b: V2, tol: f64) -> bool {
    (a.x - b.0).abs() <= tol && (a.y - b.1).abs() <= tol
}

fn check_all(w: &World, last_feet: &mut Vec<(NotchId, V2, f64)>, c: &mut Counters, label: &str) {
    let project = w.doc.project();
    let sheet = project.sheet(w.sheet).unwrap();
    assert_eq!(
        sheet.notches.len(),
        w.st.notches.len(),
        "{label} : nombre de crans"
    );
    assert!(
        !project.piece_issues(w.piece).iter().any(|i| matches!(
            i,
            PieceIssue::CutContourFailed { .. } | PieceIssue::CutNotchesFailed { .. }
        )),
        "{label} : contrôle de coupe inattendu"
    );
    for n in &w.st.notches {
        c.checks += 1;
        let stored = sheet.notch(n.id).unwrap();
        assert_eq!(
            (stored.form, stored.width_mm, stored.on_cut),
            (n.form, n.width, n.on_cut),
            "{label} : champs stockés"
        );
        let (place, segs, _) = expected(&w.st, n);
        let (got_place, got) = project.notch_placed_marks(w.sheet, stored).unwrap();
        assert_eq!(got_place, place, "{label} : lieu du cran {n:?}");
        assert_eq!(got.len(), segs.len(), "{label} : nombre de traits {n:?}");
        for (g, e) in got.iter().zip(&segs) {
            assert!(
                close(g.0, e.0, 1e-6) && close(g.1, e.1, 1e-6),
                "{label} : trait obtenu {g:?}, attendu {e:?}, cran {n:?}, état {:?}",
                w.st
            );
            c.strong_segments += 1;
        }
        if n.on_cut {
            // Aucun trait sur la couture pour un cran « sur la coupe ».
            assert!(sheet.notch_marks(stored).unwrap().is_empty(), "{label}");
            let proj = project.notch_on_cut(w.sheet, n.id).unwrap();
            let l = w.st.edge_len(n.edge);
            let p = w.st.edge_point(n.edge, n.at.clamp(0.0, l));
            let out = w.st.outward(n.edge);
            let s = cast(p, out, &w.st.cut_polygon()).unwrap();
            let foot = (p.0 + out.0 * s, p.1 + out.1 * s);
            assert!(
                close(proj.foot, foot, 1e-6),
                "{label} : pied {:?} attendu {foot:?}",
                proj.foot
            );
            assert!((proj.reach_mm - s).abs() < 1e-6, "{label} : portée");
            // CA : correct à 0,01 mm près, perpendiculaire à la couture (produit scalaire nul).
            let d = (
                proj.foot.x - proj.seam_point.x,
                proj.foot.y - proj.seam_point.y,
            );
            let t = w.st.tangent(n.edge);
            assert!(
                (d.0 * t.0 + d.1 * t.1).abs() < 0.01,
                "{label} : perpendiculaire"
            );
            c.strong_feet += 1;
            if t.0.abs() > 1e-3 && t.1.abs() > 1e-3 {
                c.slanted_feet += 1;
            }
            match w.st.mode {
                Mode::Net => c.net_checks += 1,
                Mode::Plain => c.plain_cut_checks += 1,
                Mode::Seam => {}
            }
            if let Some(prev) = last_feet.iter_mut().find(|f| f.0 == n.id) {
                if (prev.2 - w.st.margin(n.edge)).abs() > 1e-9 {
                    c.margin_followed += 1;
                }
                *prev = (n.id, foot, w.st.margin(n.edge));
            } else {
                last_feet.push((n.id, foot, w.st.margin(n.edge)));
            }
            c.forms_on_cut[form_index(n.form)] += 1;
        } else {
            c.forms_on_seam[form_index(n.form)] += 1;
        }
        if n.double {
            c.doubles += 1;
        }
    }
}

// ------------------------------------------------------------------ séquence

fn shape_ok(form: NotchForm, width: Option<f64>, depth: f64) -> Result<(), &'static str> {
    if let Some(wd) = width {
        if form == NotchForm::Line {
            return Err("width_on_line");
        }
        if !wd.is_finite() || !(0.1..=100.0).contains(&wd) {
            return Err("width_bounds");
        }
    }
    if form == NotchForm::U && width.unwrap_or(3.0) > 2.0 * depth + 1e-9 {
        return Err("u_too_wide");
    }
    Ok(())
}

fn run_sequence(index: usize, c: &mut Counters) -> Vec<String> {
    let mut rng = Rng(0xC0A7_0000 + index as u64 * 7919);
    let mut w = build(&mut rng, index);
    let mut trace = Vec::new();
    let mut last_feet: Vec<(NotchId, V2, f64)> = Vec::new();
    c.sequences += 1;
    let steps = 24;
    for step in 0..steps {
        c.steps += 1;
        let label = format!("séquence {index} pas {step}");
        let action = rng.below(100);
        let n_edges = w.st.n();
        let pick_notch = |rng: &mut Rng, st: &OState| -> Option<usize> {
            (!st.notches.is_empty()).then(|| rng.below(st.notches.len()))
        };
        match action {
            // Poser un cran sur un bord de la pièce.
            0..=24 => {
                let edge = rng.below(n_edges);
                let l = w.st.edge_len(edge);
                let at = q(rng.range(4.0, l - 4.0));
                let side = if rng.chance(50) {
                    Side::Left
                } else {
                    Side::Right
                };
                let depth = q(rng.range(1.0, 12.0));
                let double = rng.chance(25);
                let mut next = w.st.clone();
                let command = Command::AddNotch {
                    sheet: w.sheet,
                    entity: w.edges[edge],
                    distance_mm: at,
                    side,
                    length_mm: depth,
                    style: if double {
                        NotchStyle::Double
                    } else {
                        NotchStyle::Single
                    },
                };
                // L'identifiant n'est connu qu'après la commande : on le relit de la sortie.
                let placeholder = ON {
                    id: NotchId(0),
                    edge,
                    at,
                    side,
                    depth,
                    double,
                    form: NotchForm::Line,
                    width: None,
                    on_cut: false,
                };
                next.notches.push(placeholder);
                let before_len = w.st.notches.len();
                let out = w.run(command, next, c).expect("pose d'un cran valide");
                let id = out.notches[0];
                w.st.notches[before_len].id = id;
                // L'état gardé dans l'historique a été pris avant : il n'a pas ce cran, rien à corriger.
                trace.push(format!("{label} : cran {id:?}"));
            }
            // Régler la forme d'un cran existant (valide ou non).
            25..=54 => {
                let Some(k) = pick_notch(&mut rng, &w.st) else {
                    continue;
                };
                let cur = w.st.notches[k].clone();
                let form = NotchForm::ALL[rng.below(5)];
                let width = match rng.below(10) {
                    0..=3 => None,
                    4..=7 => Some(q(rng.range(0.5, 14.0))),
                    8 => Some(
                        *[0.05, 0.0, -2.0, 100.5, 1e6, f64::NAN, f64::INFINITY]
                            .get(rng.below(7))
                            .unwrap(),
                    ),
                    _ => Some(q(rng.range(5.0, 40.0))),
                };
                let on_cut = rng.chance(55);
                let command = Command::SetNotchShape {
                    sheet: w.sheet,
                    notch: cur.id,
                    form,
                    width_mm: width,
                    on_cut,
                };
                let verdict = shape_ok(form, width, cur.depth);
                let mut next = w.st.clone();
                next.notches[k].form = form;
                next.notches[k].width = width;
                next.notches[k].on_cut = on_cut;
                match (verdict, w.run(command, next, c)) {
                    (Ok(()), Ok(_)) => {}
                    (Err(kind), Err(_)) => match kind {
                        "width_on_line" => c.refused_width_on_line += 1,
                        "width_bounds" => c.refused_width_bounds += 1,
                        _ => c.refused_u_too_wide += 1,
                    },
                    (Ok(()), Err(e)) => panic!("{label} : refus inattendu : {e}"),
                    (Err(kind), Ok(_)) => panic!("{label} : accepté alors que {kind}"),
                }
            }
            // Changer la marge, la coupe nette ou retirer la marge.
            55..=74 => {
                let which = rng.below(4);
                let mut next = w.st.clone();
                match which {
                    0 | 1 => {
                        next.mode = Mode::Seam;
                        next.default = q(rng.range(3.0, 12.0));
                        next.over = (0..n_edges)
                            .map(|_| rng.chance(40).then(|| q(rng.range(2.0, 14.0))))
                            .collect();
                        let command = w.seam_command(&next);
                        match (w.st.mode, w.run(command, next, c)) {
                            (Mode::Net, Err(_)) => c.refused_seam_on_net += 1,
                            (Mode::Net, Ok(_)) => {
                                panic!("{label} : marge acceptée sur coupe nette")
                            }
                            (_, Ok(_)) => {}
                            (_, Err(e)) => panic!("{label} : marge refusée : {e}"),
                        }
                    }
                    2 => {
                        // Retirer la marge (seulement s'il y en a une : sinon no-op sans intérêt).
                        if w.st.mode != Mode::Seam {
                            continue;
                        }
                        next.mode = Mode::Plain;
                        next.default = 0.0;
                        next.over = vec![None; n_edges];
                        let command = Command::SetSeamAllowance {
                            piece: w.piece,
                            seam: None,
                        };
                        w.run(command, next, c).expect("retrait de la marge");
                    }
                    _ => {
                        let want = rng.chance(60);
                        let command = Command::SetNetCut {
                            piece: w.piece,
                            net_cut: want,
                        };
                        let result = if want {
                            if w.st.mode == Mode::Seam {
                                None
                            } else {
                                next.mode = Mode::Net;
                                Some(next)
                            }
                        } else {
                            if w.st.mode == Mode::Net {
                                next.mode = Mode::Plain;
                            }
                            Some(next)
                        };
                        match result {
                            Some(next) => {
                                w.run(command, next, c).expect("coupe nette");
                            }
                            None => {
                                let keep = w.st.clone();
                                assert!(w.run(command, keep, c).is_err(), "{label}");
                                c.refused_net_on_seam += 1;
                            }
                        }
                    }
                }
            }
            // Changer la profondeur d'un cran (refus si un U ne tient plus, ou valeur hors bornes).
            75..=82 => {
                let Some(k) = pick_notch(&mut rng, &w.st) else {
                    continue;
                };
                let cur = w.st.notches[k].clone();
                let depth = match rng.below(6) {
                    0 => 0.0,
                    1 => 150.0,
                    _ => q(rng.range(1.0, 12.0)),
                };
                let command = Command::SetNotch {
                    sheet: w.sheet,
                    notch: cur.id,
                    distance_mm: cur.at,
                    side: cur.side,
                    length_mm: depth,
                    style: if cur.double {
                        NotchStyle::Double
                    } else {
                        NotchStyle::Single
                    },
                };
                let valid =
                    depth > 0.0 && depth <= 100.0 && shape_ok(cur.form, cur.width, depth).is_ok();
                let mut next = w.st.clone();
                next.notches[k].depth = depth;
                match (valid, w.run(command, next, c)) {
                    (true, Ok(_)) => {}
                    (false, Err(_)) => c.refused_depth += 1,
                    (true, Err(e)) => panic!("{label} : profondeur refusée : {e}"),
                    (false, Ok(_)) => panic!("{label} : profondeur {depth} acceptée"),
                }
            }
            // Supprimer un cran.
            83..=86 => {
                let Some(k) = pick_notch(&mut rng, &w.st) else {
                    continue;
                };
                let id = w.st.notches[k].id;
                let mut next = w.st.clone();
                next.notches.remove(k);
                last_feet.retain(|f| f.0 != id);
                w.run(
                    Command::DeleteNotch {
                        sheet: w.sheet,
                        notch: id,
                    },
                    next,
                    c,
                )
                .expect("suppression d'un cran");
            }
            // Cran « sur la coupe » hors de toute pièce : refusé.
            87..=89 => {
                // Sur une copie du projet (l'historique du document reste intact).
                let mut p = w.doc.project().clone();
                let id = p
                    .apply(Command::AddNotch {
                        sheet: w.sheet,
                        entity: w.free,
                        distance_mm: 10.0,
                        side: Side::Left,
                        length_mm: 4.0,
                        style: NotchStyle::Single,
                    })
                    .expect("cran sur un tracé libre")
                    .notches[0];
                let (before, text) = (p.clone(), bytes(&p));
                let err = p.apply(Command::SetNotchShape {
                    sheet: w.sheet,
                    notch: id,
                    form: NotchForm::V,
                    width_mm: None,
                    on_cut: true,
                });
                assert!(err.is_err(), "{label} : sur la coupe hors pièce accepté");
                assert_eq!(p, before);
                assert_eq!(bytes(&p), text);
                c.refused_off_piece += 1;
            }
            // Annuler / rétablir.
            90..=95 => {
                if rng.chance(60) {
                    match w.past.pop() {
                        Some((text, st)) => {
                            let now = (bytes(w.doc.project()), w.st.clone());
                            assert!(w.doc.undo(), "{label} : annulation possible");
                            assert_eq!(bytes(w.doc.project()), text, "{label} : annulation exacte");
                            w.future.push(now);
                            w.st = st;
                            c.undos += 1;
                            // Une fois sur deux, on rétablit aussitôt : retour exact à l'état d'avant.
                            if rng.chance(50) {
                                let (text, st) = w.future.pop().unwrap();
                                let now = (bytes(w.doc.project()), w.st.clone());
                                assert!(w.doc.redo(), "{label} : rétablissement possible");
                                assert_eq!(
                                    bytes(w.doc.project()),
                                    text,
                                    "{label} : rétablissement"
                                );
                                w.past.push(now);
                                w.st = st;
                                c.redos += 1;
                            }
                        }
                        None => assert!(!w.doc.can_undo(), "{label}"),
                    }
                } else {
                    match w.future.pop() {
                        Some((text, st)) => {
                            let now = (bytes(w.doc.project()), w.st.clone());
                            assert!(w.doc.redo(), "{label} : rétablissement possible");
                            assert_eq!(
                                bytes(w.doc.project()),
                                text,
                                "{label} : rétablissement exact"
                            );
                            w.past.push(now);
                            w.st = st;
                            c.redos += 1;
                        }
                        None => assert!(!w.doc.can_redo(), "{label}"),
                    }
                }
            }
            // Aller-retour JSON.
            _ => {
                let text = bytes(w.doc.project());
                let back = persist::from_json(&text).expect("relecture");
                assert_eq!(back, *w.doc.project(), "{label} : relecture (valeur)");
                assert_eq!(bytes(&back), text, "{label} : relecture (octets)");
                c.roundtrips += 1;
            }
        }
        check_all(&w, &mut last_feet, c, &label);
    }
    trace
}

#[test]
fn campagne_adverse_crans_160_sequences() {
    let mut c = Counters::default();
    let mut all = Vec::new();
    for index in 0..160 {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut local = Counters::default();
            let t = run_sequence(index, &mut local);
            (t, local)
        }));
        match result {
            Ok((t, local)) => {
                all.push(t);
                c.sequences += local.sequences;
                c.steps += local.steps;
                c.checks += local.checks;
                c.strong_feet += local.strong_feet;
                c.slanted_feet += local.slanted_feet;
                c.margin_followed += local.margin_followed;
                c.strong_segments += local.strong_segments;
                for i in 0..5 {
                    c.forms_on_cut[i] += local.forms_on_cut[i];
                    c.forms_on_seam[i] += local.forms_on_seam[i];
                }
                c.doubles += local.doubles;
                c.net_checks += local.net_checks;
                c.plain_cut_checks += local.plain_cut_checks;
                c.refused_width_on_line += local.refused_width_on_line;
                c.refused_width_bounds += local.refused_width_bounds;
                c.refused_u_too_wide += local.refused_u_too_wide;
                c.refused_off_piece += local.refused_off_piece;
                c.refused_seam_on_net += local.refused_seam_on_net;
                c.refused_net_on_seam += local.refused_net_on_seam;
                c.refused_depth += local.refused_depth;
                c.undos += local.undos;
                c.redos += local.redos;
                c.noops += local.noops;
                c.roundtrips += local.roundtrips;
            }
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                panic!("séquence {index} : {msg}");
            }
        }
    }
    // Déterminisme : deux exécutions de la même graine donnent la même trace.
    let again = {
        let mut local = Counters::default();
        run_sequence(7, &mut local)
    };
    assert_eq!(again, all[7], "résultat identique à graine égale");

    eprintln!(
        "séquences {} pas {} contrôles {} pieds {} (inclinés {}, marge suivie {}) segments {} \
         formes/coupe {:?} formes/couture {:?} doubles {} coupe nette {} sans marge {} refus \
         [largeur/trait {}, largeur/bornes {}, U {}, hors pièce {}, marge/net {}, net/marge {}, \
         profondeur {}] annulations {} rétablissements {} no-ops {} aller-retours {}",
        c.sequences,
        c.steps,
        c.checks,
        c.strong_feet,
        c.slanted_feet,
        c.margin_followed,
        c.strong_segments,
        c.forms_on_cut,
        c.forms_on_seam,
        c.doubles,
        c.net_checks,
        c.plain_cut_checks,
        c.refused_width_on_line,
        c.refused_width_bounds,
        c.refused_u_too_wide,
        c.refused_off_piece,
        c.refused_seam_on_net,
        c.refused_net_on_seam,
        c.refused_depth,
        c.undos,
        c.redos,
        c.noops,
        c.roundtrips
    );
    // Les contrôles forts ont réellement été exécutés.
    assert_eq!(c.sequences, 160);
    assert!(c.steps >= 3500, "{}", c.steps);
    assert!(c.strong_feet >= 1500, "pieds comparés : {}", c.strong_feet);
    assert!(c.slanted_feet >= 800, "bords inclinés : {}", c.slanted_feet);
    assert!(
        c.margin_followed >= 150,
        "marge suivie : {}",
        c.margin_followed
    );
    assert!(
        c.strong_segments >= 20_000,
        "segments : {}",
        c.strong_segments
    );
    for i in 0..5 {
        assert!(
            c.forms_on_cut[i] >= 100,
            "forme {i} sur la coupe : {:?}",
            c.forms_on_cut
        );
        assert!(
            c.forms_on_seam[i] >= 100,
            "forme {i} sur la couture : {:?}",
            c.forms_on_seam
        );
    }
    assert!(c.doubles >= 300 && c.net_checks >= 50 && c.plain_cut_checks >= 50);
    assert!(c.refused_width_on_line >= 20, "{}", c.refused_width_on_line);
    assert!(c.refused_width_bounds >= 20, "{}", c.refused_width_bounds);
    assert!(c.refused_u_too_wide >= 20, "{}", c.refused_u_too_wide);
    assert!(c.refused_off_piece >= 20, "{}", c.refused_off_piece);
    assert!(c.refused_seam_on_net >= 20, "{}", c.refused_seam_on_net);
    assert!(c.refused_net_on_seam >= 20, "{}", c.refused_net_on_seam);
    assert!(c.refused_depth >= 20, "{}", c.refused_depth);
    assert!(c.undos >= 100 && c.redos >= 20, "{} {}", c.undos, c.redos);
    assert!(c.noops >= 20, "no-ops : {}", c.noops);
    assert!(c.roundtrips >= 50, "{}", c.roundtrips);
}
