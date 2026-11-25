//! US-05.1 — campagne adverse du contour de coupe : 160 séquences à graine fixe (polygones
//! étoilés à 3 à 8 sommets, pointes aiguës, marges par défaut, surcharges par bord, marges
//! progressives, déplacements de sommets, annuler / rétablir, commandes invalides).
//!
//! Oracle **indépendant** : aucune fonction de `pf_geometry::seam` ni de `piece_cut_contour` n'y
//! sert à juger le résultat. Les polygones sont des segments ; l'aire (formule du lacet), les
//! distances point-segment et les croisements sont recalculés ici. Propriétés :
//!
//! 1. jamais de panique, résultat identique à graine égale (déterminisme) ;
//! 2. contour de coupe fermé, formé de segments, sans croisement, aire ≥ aire de couture ;
//! 3. aucun point du contour de coupe plus près de la couture que la plus petite marge, ni plus
//!    loin que 4 × la plus grande (limite de pointe : une pointe très aiguë est coupée en biseau) ;
//! 4. le point du décalage au milieu de chaque bord (marge moyenne `(début + fin) / 2`, vers
//!    l'extérieur) est sur le contour de coupe quand aucun autre bord n'en est à moins de la plus
//!    grande marge (signe, début / fin et progression sont ainsi vérifiés) ;
//! 5. annuler rend le contour de couture exact, rétablir rend le même contour de coupe (bit à
//!    bit) ; déplacer un sommet recalcule le contour de coupe sans autre commande ;
//! 6. échec de calcul = contrôle `CutContourFailed`, jamais un refus ; commande invalide =
//!    refus sans trace (octets identiques).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Trace};
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

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Arrondi au centième de mm : des sommets « propres » (pas de points confondus par hasard).
fn q(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ------------------------------------------------------------------ oracle

fn dist_seg(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let (u, w) = (b - a, p - a);
    let len2 = u.dot(u);
    let t = if len2 == 0.0 {
        0.0
    } else {
        (w.dot(u) / len2).clamp(0.0, 1.0)
    };
    let foot = pt(a.x + u.x * t, a.y + u.y * t);
    ((p.x - foot.x).powi(2) + (p.y - foot.y).powi(2)).sqrt()
}

fn dist_poly(p: PointMm, v: &[PointMm]) -> f64 {
    (0..v.len())
        .map(|i| dist_seg(p, v[i], v[(i + 1) % v.len()]))
        .fold(f64::MAX, f64::min)
}

fn shoelace(v: &[PointMm]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].x * v[(i + 1) % n].y - v[(i + 1) % n].x * v[i].y)
        .sum::<f64>()
        / 2.0
}

fn crossing(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    let (u, w) = (b - a, d - c);
    let den = u.cross(w);
    if den.abs() < 1e-12 {
        return false;
    }
    let t = (c - a).cross(w) / den;
    let s = (c - a).cross(u) / den;
    let e = 1e-7;
    t > e && t < 1.0 - e && s > e && s < 1.0 - e
}

// ------------------------------------------------------------------ scène

struct Scene {
    p: Project,
    sheet: SheetId,
    piece: PieceId,
    /// Sommets anti-horaires ; le bord `i` va de `v[i]` à `v[i + 1]`.
    v: Vec<PointMm>,
    edges: Vec<EntityId>,
    /// Pointe aiguë (test du biseau).
    spike: bool,
}

fn build(rng: &mut Rng, index: usize) -> Scene {
    let spike = index % 6 == 0;
    let v: Vec<PointMm> = if spike {
        // Sommet en (0,0), demi-angle de 3° à 14° : pointe > 4 × la marge, donc biseau.
        let half = rng.range(3.0, 14.0).to_radians();
        let len = rng.range(80.0, 120.0);
        let (cx, h) = (len * half.cos(), len * half.sin());
        vec![pt(0.0, 0.0), pt(q(cx), q(-h)), pt(q(cx), q(h))]
    } else {
        let n = 3 + rng.below(6);
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * (i as f64 + rng.range(-0.25, 0.25)) / n as f64;
                let r = rng.range(40.0, 100.0);
                pt(q(r * a.cos()), q(r * a.sin()))
            })
            .collect()
    };
    let mut p = Project::new("Adverse marges");
    let sheet = p.sheets[0].id;
    let n = v.len();
    let edges: Vec<EntityId> = (0..n)
        .map(|i| {
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(v[i]),
                to: PointRef::New(v[(i + 1) % n]),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Étoile".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Scene {
        p,
        sheet,
        piece,
        v,
        edges,
        spike,
    }
}

fn random_seam(rng: &mut Rng, s: &Scene) -> SeamAllowance {
    let top = if s.spike { 10.0 } else { 14.0 };
    let low = if s.spike { 2.0 } else { 0.0 };
    let default_mm = if !s.spike && rng.chance(15) {
        0.0
    } else {
        q(rng.range(low, top))
    };
    let mut edges = Vec::new();
    // 35 % des séquences : marge uniforme (chemin `offset_chain`), sans surcharge.
    if !rng.chance(35) {
        for &entity in &s.edges {
            if rng.chance(45) {
                let start_mm = if rng.chance(15) {
                    0.0
                } else {
                    q(rng.range(low, top))
                };
                let end_mm = if rng.chance(35) {
                    start_mm
                } else {
                    q(rng.range(low, top))
                };
                edges.push(EdgeAllowance {
                    entity,
                    start_mm,
                    end_mm,
                });
            }
        }
    }
    SeamAllowance {
        default_mm,
        edges,
        ..Default::default()
    }
}

fn margins(s: &Scene, seam: Option<&SeamAllowance>) -> Vec<(f64, f64)> {
    s.edges
        .iter()
        .map(|e| seam.map_or((0.0, 0.0), |m| m.margin_of(*e)))
        .collect()
}

fn d2(a: PointMm, b: PointMm) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Retire les doublons et les points alignés d'un polygone (liste cyclique).
fn clean(pts: &[PointMm]) -> Vec<PointMm> {
    let mut v: Vec<PointMm> = Vec::new();
    for p in pts {
        if v.last().is_none_or(|l| d2(*l, *p) > 1e-9) {
            v.push(*p);
        }
    }
    while v.len() > 1 && d2(v[0], v[v.len() - 1]) <= 1e-9 {
        v.pop();
    }
    loop {
        let n = v.len();
        let drop = (0..n).find(|&i| {
            let (a, b, c) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
            let (u, w) = (b - a, c - b);
            u.cross(w).abs() <= 1e-9 * u.norm() * w.norm() && u.dot(w) > 0.0
        });
        match drop {
            Some(i) if n > 3 => {
                v.remove(i);
            }
            _ => break,
        }
    }
    v
}

fn same_cycle(a: &[PointMm], b: &[PointMm], tol: f64) -> bool {
    a.len() == b.len()
        && (0..a.len()).any(|s| (0..a.len()).all(|i| d2(a[(i + s) % a.len()], b[i]) <= tol))
}

/// Polygone attendu du contour de coupe d'un polygone de segments : sommet = intersection des
/// deux droites décalées voisines ; pointe d'un coin extérieur au-delà de 4 × la plus grande marge
/// adjacente = biseau (bout du décalage précédent puis début du suivant). `None` quand la
/// construction naïve n'est pas valide (droites parallèles, bord retourné, croisement, horaire) :
/// le résultat de la bibliothèque n'est alors pas jugé par cette voie.
fn expected_polygon(v: &[PointMm], m: &[(f64, f64)]) -> Option<Vec<PointMm>> {
    let n = v.len();
    // Droite décalée de chaque bord : (début, fin, direction unitaire).
    let lines: Vec<(PointMm, PointMm, PointMm)> = (0..n)
        .map(|i| {
            let (a, b) = (v[i], v[(i + 1) % n]);
            let len = d2(a, b);
            let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
            let (rx, ry) = (uy, -ux);
            let p = pt(a.x + rx * m[i].0, a.y + ry * m[i].0);
            let q = pt(b.x + rx * m[i].1, b.y + ry * m[i].1);
            let l = d2(p, q);
            (p, q, pt((q.x - p.x) / l, (q.y - p.y) / l))
        })
        .collect();
    let mut out = Vec::new();
    let mut entering: Vec<PointMm> = Vec::new(); // début effectif de chaque décalage
    let mut leaving: Vec<PointMm> = vec![pt(0.0, 0.0); n]; // fin effective
    for j in 0..n {
        let i = (j + n - 1) % n;
        let (p, q, du) = lines[i];
        let (r, _, dw) = lines[j];
        let den = du.x * dw.y - du.y * dw.x;
        if den.abs() < 1e-9 {
            return None;
        }
        let t = ((r.x - p.x) * dw.y - (r.y - p.y) * dw.x) / den;
        let x = pt(p.x + du.x * t, p.y + du.y * t);
        let (a, b, c) = (v[i], v[j], v[(j + 1) % n]);
        let convex = (b - a).cross(c - b) > 0.0;
        let reach = 4.0 * m[i].1.max(m[j].0);
        if convex && d2(x, v[j]) > reach {
            // Biseau : bout du décalage précédent puis début du suivant.
            out.push(q);
            out.push(lines[j].0);
            leaving[i] = q;
            entering.push(lines[j].0);
        } else {
            out.push(x);
            leaving[i] = x;
            entering.push(x);
        }
    }
    // Chaque décalage doit avancer dans son sens, de son entrée vers sa sortie.
    for j in 0..n {
        let (_, _, du) = lines[j];
        let (a, b) = (entering[j], leaving[j]);
        if (b.x - a.x) * du.x + (b.y - a.y) * du.y <= 1e-6 {
            return None;
        }
    }
    let poly = clean(&out);
    if poly.len() < 3 || shoelace(&poly) <= 0.0 {
        return None;
    }
    for i in 0..poly.len() {
        for j in i + 2..poly.len() {
            if i == 0 && j == poly.len() - 1 {
                continue;
            }
            if crossing(
                poly[i],
                poly[(i + 1) % poly.len()],
                poly[j],
                poly[(j + 1) % poly.len()],
            ) {
                return None;
            }
        }
    }
    Some(poly)
}

#[derive(Default)]
struct Counters {
    polygon_checks: usize,
    sequences: usize,
    ok: usize,
    failed_issue: usize,
    uniform: usize,
    progressive: usize,
    mid_edge_checks: usize,
    spike_ok: usize,
    refused: usize,
    moves: usize,
    undo_redo: usize,
    zero: usize,
    noop: usize,
    lower_bound_checks: usize,
}

/// Juge un contour de coupe contre le polygone `s.v` et les marges `m`.
fn judge(s: &Scene, cut: &CutContour, m: &[(f64, f64)], c: &mut Counters, what: &str) {
    let n = s.v.len();
    let lines: Vec<(PointMm, PointMm)> = cut
        .traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, b) => (*a, *b),
            other => panic!("{what} : segment attendu, reçu {other:?}"),
        })
        .collect();
    assert!(lines.len() >= 3, "{what}");
    // Fermé et continu.
    for i in 0..lines.len() {
        let (a, b) = (lines[i].1, lines[(i + 1) % lines.len()].0);
        assert!(
            ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() < 1e-4,
            "{what} : contour de coupe non continu en {i}"
        );
    }
    // Aire et périmètre recalculés ici.
    let poly: Vec<PointMm> = lines.iter().map(|l| l.0).collect();
    let area_cut = shoelace(&poly);
    let area_seam = shoelace(&s.v);
    assert!(area_seam > 0.0, "{what} : couture horaire");
    assert!(
        (area_cut - cut.area_mm2).abs() < 1e-6,
        "{what} : aire annoncée {} ≠ {area_cut}",
        cut.area_mm2
    );
    assert!(
        area_cut >= area_seam - 1e-6,
        "{what} : aire de coupe {area_cut} < couture {area_seam}"
    );
    let perimeter: f64 = lines
        .iter()
        .map(|(a, b)| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt())
        .sum();
    assert!(
        (perimeter - cut.perimeter_mm).abs() < 1e-6,
        "{what} : périmètre annoncé {} ≠ {perimeter}",
        cut.perimeter_mm
    );
    // Sans croisement.
    for i in 0..lines.len() {
        for j in i + 2..lines.len() {
            if i == 0 && j == lines.len() - 1 {
                continue;
            }
            assert!(
                !crossing(lines[i].0, lines[i].1, lines[j].0, lines[j].1),
                "{what} : le contour de coupe se recoupe ({i} × {j})"
            );
        }
    }
    // Polygone attendu, construit ici (intersections de droites décalées) : il doit être celui
    // du contour de coupe dès qu'il est valide.
    if let Some(want) = expected_polygon(&s.v, m) {
        let got = clean(&poly);
        assert!(
            same_cycle(&got, &want, 1e-6),
            "{what} : contour de coupe {got:?} ≠ polygone attendu {want:?} ; couture {:?} ; marges {m:?}",
            s.v
        );
        c.polygon_checks += 1;
    }
    // Distances à la couture : plus petite marge ≤ d ≤ 4 × la plus grande.
    let all: Vec<f64> = m.iter().flat_map(|x| [x.0, x.1]).collect();
    let (lo, hi) = (
        all.iter().cloned().fold(f64::MAX, f64::min),
        all.iter().cloned().fold(0.0, f64::max),
    );
    // Borne basse : seulement pour une marge uniforme (une droite décalée inclinée, prolongée
    // au-delà de son bout, passe légitimement sous la plus petite marge) ; un biseau passe à
    // `m · cos(virage / 2)` du sommet.
    let flat = (hi - lo).abs() < 1e-12;
    let mut bevel_factor = 1.0_f64;
    for i in 0..n {
        let (a, b, d) = (s.v[i], s.v[(i + 1) % n], s.v[(i + 2) % n]);
        let (u, w) = (b - a, d - b);
        let turn = u.cross(w).atan2(u.dot(w));
        if turn > 0.0 && 1.0 / (turn / 2.0).cos() > 4.0 {
            bevel_factor = bevel_factor.min((turn / 2.0).cos());
        }
    }
    for (a, b) in &lines {
        for k in 0..=6 {
            let t = f64::from(k) / 6.0;
            let p = pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let d = dist_poly(p, &s.v);
            if flat {
                assert!(
                    d >= lo * bevel_factor - 1e-6,
                    "{what} : point à {d} < marge {lo} (× {bevel_factor})"
                );
                c.lower_bound_checks += 1;
            }
            assert!(
                d <= 4.0 * hi + 1e-6,
                "{what} : point à {d} > 4 × la marge maximale {hi} (pointe non coupée en biseau)"
            );
        }
    }
    // Point du milieu de chaque bord.
    for i in 0..n {
        let (a, b) = (s.v[i], s.v[(i + 1) % n]);
        let (ma, mb) = m[i];
        let mid = pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let dir = b - a;
        let len = dir.norm();
        // Normale extérieure (à droite d'un contour anti-horaire).
        let right = pt(dir.y / len, -dir.x / len);
        let off = (ma + mb) / 2.0;
        let expected = pt(mid.x + right.x * off, mid.y + right.y * off);
        let others = (0..n)
            .filter(|&j| j != i)
            .map(|j| dist_seg(expected, s.v[j], s.v[(j + 1) % n]))
            .fold(f64::MAX, f64::min);
        // Un voisin dont la marge diffère au sommet partagé raccourcit légitimement ce décalage
        // (onglet entre deux marges différentes) : le contrôle du milieu ne vaut que si la
        // transition est lisse ; les autres cas sont jugés par le polygone attendu (ci-dessus).
        let (pm, nm) = (m[(i + n - 1) % n], m[(i + 1) % n]);
        let smooth = (pm.1 - ma).abs() < 1e-9 && (nm.0 - mb).abs() < 1e-9;
        if smooth && others >= hi + 1e-3 {
            let d = lines
                .iter()
                .map(|(p, q)| dist_seg(expected, *p, *q))
                .fold(f64::MAX, f64::min);
            assert!(
                d < 1e-6,
                "{what} : le décalage du bord {i} (marges {ma} / {mb}) ne passe pas par {expected:?} \
                 (écart {d}) ; couture {:?} ; coupe {lines:?}",
                s.v
            );
            c.mid_edge_checks += 1;
        }
    }
}

/// Une séquence complète ; rend l'empreinte du résultat (déterminisme).
fn run(seed: u64, index: usize, c: &mut Counters) -> String {
    let mut rng = Rng(seed);
    let mut s = build(&mut rng, index);
    let what = format!("séquence {index} (graine {seed})");
    let mut doc = Document::new(s.p.clone());
    let base = persist::to_json(doc.project()).unwrap();
    let mut fingerprint = String::new();

    // Sans marge : copie exacte du contour de couture (aire, périmètre indépendants).
    let none = doc.project().piece_cut_contour(s.piece).unwrap();
    judge(&s, &none, &margins(&s, None), c, &what);
    assert!((none.area_mm2 - shoelace(&s.v)).abs() < 1e-6, "{what}");
    c.zero += 1;

    // Commandes invalides : refus sans trace.
    for bad in [
        SeamAllowance {
            default_mm: -rng.range(0.001, 5.0),
            edges: vec![],
            ..Default::default()
        },
        SeamAllowance {
            default_mm: f64::NAN,
            edges: vec![],
            ..Default::default()
        },
        SeamAllowance {
            default_mm: 1000.0 + rng.range(0.001, 5.0),
            edges: vec![],
            ..Default::default()
        },
        SeamAllowance {
            default_mm: 3.0,
            edges: vec![EdgeAllowance {
                entity: EntityId(9_000_000 + index as u64),
                start_mm: 1.0,
                end_mm: 1.0,
            }],
            ..Default::default()
        },
        SeamAllowance {
            default_mm: 3.0,
            edges: vec![
                EdgeAllowance {
                    entity: s.edges[0],
                    start_mm: 1.0,
                    end_mm: 1.0,
                },
                EdgeAllowance {
                    entity: s.edges[0],
                    start_mm: 2.0,
                    end_mm: 2.0,
                },
            ],
            ..Default::default()
        },
    ] {
        let before = persist::to_json(doc.project()).unwrap();
        let r = doc.execute(Command::SetSeamAllowance {
            piece: s.piece,
            seam: Some(bad),
        });
        assert!(r.is_err(), "{what} : commande invalide acceptée");
        assert_eq!(
            before,
            persist::to_json(doc.project()).unwrap(),
            "{what} : trace d'un refus"
        );
        c.refused += 1;
    }
    assert!(!doc.can_undo(), "{what} : un refus crée une étape");

    // Marge valide : toujours acceptée, même si le calcul échoue ensuite.
    let seam = random_seam(&mut rng, &s);
    let uniform = seam.edges.is_empty();
    let progressive = seam.edges.iter().any(|e| e.start_mm != e.end_mm);
    doc.execute(Command::SetSeamAllowance {
        piece: s.piece,
        seam: Some(seam.clone()),
    })
    .unwrap_or_else(|e| panic!("{what} : marge valide refusée : {e}"));
    let stored = doc.project().piece(s.piece).unwrap().seam.clone();
    let m = margins(&s, stored.as_ref());
    let first = doc.project().piece_cut_contour(s.piece);
    let judge_now = |doc: &Document, s: &Scene, c: &mut Counters, label: &str| {
        let project = doc.project();
        let m = margins(s, project.piece(s.piece).unwrap().seam.as_ref());
        match project.piece_cut_contour(s.piece) {
            Ok(cut) => {
                assert!(
                    !project
                        .piece_issues(s.piece)
                        .iter()
                        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. })),
                    "{label} : contrôle présent alors que le calcul réussit"
                );
                judge(s, &cut, &m, c, label);
                c.ok += 1;
                if s.spike {
                    c.spike_ok += 1;
                }
                Some(cut)
            }
            Err(e) => {
                assert!(
                    expected_polygon(&s.v, &m).is_none(),
                    "{label} : calcul impossible ({e}) alors que le polygone attendu est valide : \
                     couture {:?}, marges {m:?}, attendu {:?}",
                    s.v,
                    expected_polygon(&s.v, &m)
                );
                assert!(
                    project
                        .piece_issues(s.piece)
                        .iter()
                        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. })),
                    "{label} : échec de calcul sans contrôle : {e}"
                );
                c.failed_issue += 1;
                None
            }
        }
    };
    let after_set = judge_now(&doc, &s, c, &what);
    if uniform {
        c.uniform += 1;
    }
    if progressive {
        c.progressive += 1;
    }
    fingerprint.push_str(&format!("{m:?}|{first:?}|"));

    // Annuler : contour de couture exact ; rétablir : même contour de coupe, bit à bit.
    if stored.is_none() {
        // Marge vide : rien n'est stocké, aucune étape.
        assert!(!doc.can_undo(), "{what} : une marge vide crée une étape");
        assert_eq!(base, persist::to_json(doc.project()).unwrap(), "{what}");
        c.noop += 1;
    } else {
        assert!(doc.undo(), "{what}");
        assert_eq!(
            base,
            persist::to_json(doc.project()).unwrap(),
            "{what} : annuler"
        );
        let back = doc.project().piece_cut_contour(s.piece).unwrap();
        assert_eq!(
            back.traces, none.traces,
            "{what} : annuler rend la copie de la couture"
        );
        assert!(doc.redo(), "{what}");
        let again = doc.project().piece_cut_contour(s.piece).ok();
        assert_eq!(again, after_set, "{what} : rétablir donne un autre contour");
        c.undo_redo += 1;
    }

    // Déplacements de sommets : le contour de coupe suit sans autre commande.
    if !s.spike {
        for _ in 0..(1 + rng.below(3)) {
            let k = rng.below(s.v.len());
            let r = (s.v[k].x.powi(2) + s.v[k].y.powi(2)).sqrt();
            let scale = (r * rng.range(0.85, 1.15)).clamp(35.0, 110.0) / r;
            let to = pt(q(s.v[k].x * scale), q(s.v[k].y * scale));
            let id = {
                let sheet = doc.project().sheet(s.sheet).unwrap();
                sheet
                    .points
                    .iter()
                    .find(|p| {
                        let at = sheet.position(p.id).unwrap();
                        (at.x - s.v[k].x).abs() < 1e-9 && (at.y - s.v[k].y).abs() < 1e-9
                    })
                    .unwrap_or_else(|| panic!("{what} : sommet {k} introuvable"))
                    .id
            };
            let before = persist::to_json(doc.project()).unwrap();
            let seam_before = doc.project().piece(s.piece).unwrap().seam.clone();
            match doc.execute(Command::MovePoint {
                sheet: s.sheet,
                point: id,
                to,
            }) {
                Ok(_) => {
                    s.v[k] = to;
                    c.moves += 1;
                    assert_eq!(
                        doc.project().piece(s.piece).unwrap().seam,
                        seam_before,
                        "{what} : déplacer un point ne touche pas la marge"
                    );
                    let cut = judge_now(&doc, &s, c, &format!("{what}, après déplacement"));
                    fingerprint.push_str(&format!("{cut:?}|"));
                }
                Err(_) => assert_eq!(before, persist::to_json(doc.project()).unwrap(), "{what}"),
            }
        }
    }
    fingerprint
}

#[test]
fn campagne_adverse_marges_de_couture_160_sequences() {
    let mut c = Counters::default();
    let mut prints = Vec::new();
    for index in 0..160usize {
        let seed = 0xA11C_E000_5EA1_0000u64 ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        c.sequences += 1;
        let result = catch_unwind(AssertUnwindSafe(|| run(seed, index, &mut c)));
        match result {
            Ok(f) => prints.push((seed, index, f)),
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                panic!("séquence {index} (graine {seed}) : {msg}");
            }
        }
    }
    // Déterminisme : quelques séquences rejouées donnent la même empreinte, bit à bit.
    for (seed, index, f) in prints.iter().step_by(13) {
        let mut sink = Counters::default();
        assert_eq!(
            &run(*seed, *index, &mut sink),
            f,
            "séquence {index} non déterministe"
        );
    }
    // Les contrôles forts ont bien été exécutés (l'oracle ne passe pas à vide).
    println!(
        "polygones attendus {} ; bornes basses {} ; séquences {} ; ok {} ; échecs signalés {} ; uniformes {} ; progressives {} ; contrôles \
         milieu de bord {} ; pointes ok {} ; refus {} ; déplacements {} ; annuler/rétablir {}",
        c.polygon_checks,
        c.lower_bound_checks,
        c.sequences,
        c.ok,
        c.failed_issue,
        c.uniform,
        c.progressive,
        c.mid_edge_checks,
        c.spike_ok,
        c.refused,
        c.moves,
        c.undo_redo
    );
    assert_eq!(c.sequences, 160);
    assert!(c.ok >= 150, "peu de contours calculés : {}", c.ok);
    assert!(c.uniform >= 30, "chemin uniforme : {}", c.uniform);
    assert!(
        c.progressive >= 25,
        "marges progressives : {}",
        c.progressive
    );
    assert!(
        c.polygon_checks >= 150,
        "polygones attendus : {}",
        c.polygon_checks
    );
    assert!(
        c.mid_edge_checks >= 150,
        "contrôles de bord : {}",
        c.mid_edge_checks
    );
    assert!(c.spike_ok >= 20, "pointes aiguës : {}", c.spike_ok);
    assert!(c.refused >= 700, "refus : {}", c.refused);
    assert!(c.moves >= 150, "déplacements : {}", c.moves);
    assert_eq!(c.undo_redo + c.noop, 160);
    assert!(c.undo_redo >= 120, "annuler/rétablir : {}", c.undo_redo);
    assert_eq!(c.zero, 160);
    assert!(
        c.lower_bound_checks >= 1000,
        "bornes basses : {}",
        c.lower_bound_checks
    );
}
