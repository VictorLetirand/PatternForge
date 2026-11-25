//! US-05.6 — campagne adverse de la « coupe nette » : 160 séquences à graine fixe (polygones
//! étoilés à 3 à 8 sommets ou disque ; déclarer / retirer la coupe nette, poser / retirer une
//! marge, déplacer un sommet, annuler / rétablir, aller-retour JSON, commandes refusées).
//!
//! Oracle **indépendant** : un petit automate (`net`, `seam`) tient l'état attendu de la pièce et
//! décide seul quelles commandes doivent être refusées ; l'aire (formule du lacet), le périmètre
//! (somme des côtés), le cercle (π r²) et l'historique d'annulation (octets du fichier) sont
//! recalculés ici sans aucune fonction de `piece_cut_contour` ni de `pf_geometry::seam`. Propriétés :
//!
//! 1. jamais de panique, résultat identique à graine égale (déterminisme) ;
//! 2. exclusion : coupe nette sur une pièce à marge, ou marge sur une coupe nette = refus, projet
//!    identique à l'octet ; tout le reste réussit ;
//! 3. coupe nette (et pièce sans marge) : contour de coupe = polygone d'origine (mêmes sommets,
//!    périmètre, aire de l'oracle), recalculé après chaque déplacement de sommet ;
//! 4. le champ `net_cut` est écrit si et seulement si la pièce est déclarée ; relecture exacte
//!    (valeur et octets) ;
//! 5. annuler / rétablir : exactement une étape par changement réel, aucune pour un no-op.

use pf_core::persist;
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

/// Arrondi au centième de mm : des sommets « propres ».
fn q(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ------------------------------------------------------------------ oracle

fn shoelace(v: &[PointMm]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].x * v[(i + 1) % n].y - v[(i + 1) % n].x * v[i].y)
        .sum::<f64>()
        / 2.0
}

fn perimeter(v: &[PointMm]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| {
            let (a, b) = (v[i], v[(i + 1) % n]);
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
        })
        .sum()
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

fn simple(v: &[PointMm]) -> bool {
    let n = v.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if crossing(v[i], v[(i + 1) % n], v[j], v[(j + 1) % n]) {
                return false;
            }
        }
    }
    shoelace(v) > 1.0
}

/// Distance minimale entre deux sommets : écarte les déplacements qui confondent des points.
fn spread(v: &[PointMm]) -> bool {
    let n = v.len();
    (0..n).all(|i| {
        (i + 1..n).all(|j| ((v[i].x - v[j].x).powi(2) + (v[i].y - v[j].y).powi(2)).sqrt() > 1.0)
    })
}

// ------------------------------------------------------------------ scène

#[derive(Default)]
struct Counters {
    sequences: usize,
    steps: usize,
    declared: usize,
    undeclared: usize,
    refused_net_on_seam: usize,
    refused_seam_on_net: usize,
    seams_set: usize,
    noops: usize,
    moves: usize,
    undos: usize,
    redos: usize,
    roundtrips: usize,
    /// Contrôles forts du contour de coupe contre l'oracle (coupe nette ou sans marge).
    strong_cuts: usize,
    circles: usize,
    unknown_piece: usize,
}

struct Scene {
    doc: Document,
    sheet: SheetId,
    piece: PieceId,
    /// Sommets du polygone (vide pour le disque).
    v: Vec<PointMm>,
    radius: f64,
}

fn build(rng: &mut Rng, index: usize) -> Scene {
    let mut p = Project::new("Adverse coupe nette");
    let sheet = p.sheets[0].id;
    let (v, radius, edges) = if index % 5 == 4 {
        let r = q(rng.range(10.0, 60.0));
        let c = p
            .apply(Command::AddCircle {
                sheet,
                center: PointRef::New(pt(0.0, 0.0)),
                radius_mm: r,
            })
            .unwrap()
            .entities[0];
        (Vec::new(), r, vec![c])
    } else {
        let n = 3 + rng.below(6);
        let v: Vec<PointMm> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * (i as f64 + rng.range(-0.25, 0.25)) / n as f64;
                let r = rng.range(40.0, 100.0);
                pt(q(r * a.cos()), q(r * a.sin()))
            })
            .collect();
        let edges = (0..n)
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
        (v, 0.0, edges)
    };
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities: edges },
            name: "Cuir".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Scene {
        doc: Document::new(p),
        sheet,
        piece,
        v,
        radius,
    }
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

/// Contrôle fort : le contour de coupe est celui de l'oracle.
fn check_cut_equals_sewing(s: &Scene, c: &mut Counters) {
    let cut = s
        .doc
        .project()
        .piece_cut_contour(s.piece)
        .expect("contour de coupe d'une pièce sans marge");
    if s.v.is_empty() {
        let (area, per) = (
            std::f64::consts::PI * s.radius * s.radius,
            std::f64::consts::TAU * s.radius,
        );
        assert!(
            (cut.area_mm2 - area).abs() < 1e-6 * area.max(1.0),
            "aire du disque"
        );
        assert!(
            (cut.perimeter_mm - per).abs() < 1e-6 * per.max(1.0),
            "périmètre du disque"
        );
    } else {
        assert!(
            (cut.area_mm2 - shoelace(&s.v).abs()).abs() < 1e-6,
            "aire {} contre {}",
            cut.area_mm2,
            shoelace(&s.v).abs()
        );
        assert!(
            (cut.perimeter_mm - perimeter(&s.v)).abs() < 1e-6,
            "périmètre"
        );
        // Mêmes sommets : chaque segment de coupe commence en un sommet du polygone, une fois.
        assert_eq!(cut.traces.len(), s.v.len());
        let mut used = vec![false; s.v.len()];
        for t in &cut.traces {
            let Trace::Line(a, _) = t else {
                panic!("segment attendu, reçu {t:?}")
            };
            let k = s
                .v
                .iter()
                .enumerate()
                .position(|(i, w)| !used[i] && (w.x - a.x).abs() < 1e-9 && (w.y - a.y).abs() < 1e-9)
                .expect("sommet de coupe absent du polygone");
            used[k] = true;
        }
    }
    c.strong_cuts += 1;
}

/// Vérifie l'état du projet contre l'automate, le fichier et la coupe.
fn check_state(s: &Scene, net: bool, seam: Option<f64>, c: &mut Counters) {
    let p = s.doc.project();
    let piece = p.piece(s.piece).unwrap();
    assert_eq!(piece.net_cut, net, "déclaration");
    assert_eq!(
        piece.seam.as_ref().map(|x| x.default_mm),
        seam,
        "marge enregistrée"
    );
    assert!(!(piece.net_cut && piece.seam.is_some()), "exclusion");
    assert!(p.validate().is_ok(), "{:?}", p.validate());
    let text = bytes(p);
    assert_eq!(text.contains("\"net_cut\""), net, "clé écrite ssi déclarée");
    let back = persist::from_json(&text).expect("relecture");
    assert_eq!(&back, p, "relecture exacte");
    assert_eq!(bytes(&back), text, "octets stables");
    c.roundtrips += 1;
    if net || seam.is_none() {
        check_cut_equals_sewing(s, c);
    }
}

fn run_sequence(index: usize, c: &mut Counters) -> Vec<String> {
    let mut rng = Rng(0x5EED_0560 + index as u64 * 7919);
    let mut s = build(&mut rng, index);
    c.sequences += 1;
    if s.v.is_empty() {
        c.circles += 1;
    }
    let (mut net, mut seam): (bool, Option<f64>) = (false, None);
    // Historique attendu (octets) : une entrée par étape réelle, indépendante du document.
    let mut history = vec![bytes(s.doc.project())];
    let mut cursor = 0usize;
    let mut trace = Vec::new();
    check_state(&s, net, seam, c);
    for _ in 0..(6 + rng.below(5)) {
        c.steps += 1;
        let before = bytes(s.doc.project());
        let op = rng.below(10);
        trace.push(format!("op{op}"));
        // Effet attendu : Ok(changement ?) ou refus.
        let mut expect_refusal = false;
        // Raison attendue du refus : la commande elle-même (pas la validation globale du projet).
        let mut reason = "";
        let command = match op {
            0 | 1 => {
                let want = op == 0 || rng.chance(30);
                if want && seam.is_some() {
                    expect_refusal = true;
                    reason = "retirez d'abord la marge";
                    c.refused_net_on_seam += 1;
                }
                Some(Command::SetNetCut {
                    piece: s.piece,
                    net_cut: want,
                })
            }
            2 | 3 => {
                let m = if rng.chance(25) {
                    0.0
                } else {
                    q(rng.range(1.0, 20.0))
                };
                // Une marge nulle sans surcharge n'est pas une marge (rien n'est stocké).
                if net && m > 0.0 {
                    expect_refusal = true;
                    reason = "retirez d'abord la coupe nette";
                    c.refused_seam_on_net += 1;
                }
                Some(Command::SetSeamAllowance {
                    piece: s.piece,
                    seam: Some(SeamAllowance {
                        default_mm: m,
                        ..Default::default()
                    }),
                })
            }
            4 => Some(Command::SetSeamAllowance {
                piece: s.piece,
                seam: None,
            }),
            5 => {
                if s.v.is_empty() {
                    None
                } else {
                    // Déplacement d'un sommet, accepté seulement si le polygone reste simple.
                    let i = rng.below(s.v.len());
                    let to = pt(
                        q(s.v[i].x + rng.range(-6.0, 6.0)),
                        q(s.v[i].y + rng.range(-6.0, 6.0)),
                    );
                    let mut w = s.v.clone();
                    w[i] = to;
                    if simple(&w) && spread(&w) {
                        let sheet = s.doc.project().sheet(s.sheet).unwrap();
                        let id = sheet
                            .points
                            .iter()
                            .find(|x| {
                                let at = sheet.position(x.id).unwrap();
                                (at.x - s.v[i].x).abs() < 1e-9 && (at.y - s.v[i].y).abs() < 1e-9
                            })
                            .map(|x| x.id);
                        id.map(|point| {
                            s.v = w;
                            c.moves += 1;
                            Command::MovePoint {
                                sheet: s.sheet,
                                point,
                                to,
                            }
                        })
                    } else {
                        None
                    }
                }
            }
            6 => {
                // Pièce inconnue : toujours refusé, aucune trace.
                c.unknown_piece += 1;
                expect_refusal = true;
                reason = "";
                Some(Command::SetNetCut {
                    piece: PieceId(990_000 + index as u64),
                    net_cut: true,
                })
            }
            _ => None,
        };
        if let Some(cmd) = command {
            let is_declare_true =
                matches!(cmd, Command::SetNetCut { net_cut: true, .. }) && !expect_refusal && !net;
            let is_declare_false = matches!(cmd, Command::SetNetCut { net_cut: false, .. }) && net;
            let seam_target = match &cmd {
                Command::SetSeamAllowance { seam: Some(a), .. } => Some(a.default_mm),
                _ => None,
            };
            let removes_seam = matches!(cmd, Command::SetSeamAllowance { seam: None, .. });
            let result = s.doc.execute(cmd);
            if expect_refusal {
                let err = result.expect_err("refus attendu");
                assert!(
                    err.to_string().contains(reason),
                    "séquence {index} {trace:?} : « {err} » ne contient pas « {reason} »"
                );
                assert_eq!(bytes(s.doc.project()), before, "refus sans trace");
            } else {
                result.unwrap_or_else(|e| panic!("séquence {index} {trace:?} : {e}"));
                if is_declare_true {
                    net = true;
                    c.declared += 1;
                }
                if is_declare_false {
                    net = false;
                    c.undeclared += 1;
                }
                if let Some(m) = seam_target {
                    seam = (m > 0.0).then_some(m);
                    c.seams_set += 1;
                }
                if removes_seam {
                    seam = None;
                }
                let after = bytes(s.doc.project());
                if after == before {
                    c.noops += 1;
                } else {
                    history.truncate(cursor + 1);
                    history.push(after);
                    cursor += 1;
                }
            }
        } else if op == 7 && cursor > 0 {
            assert!(s.doc.undo(), "annulation possible");
            cursor -= 1;
            c.undos += 1;
            assert_eq!(bytes(s.doc.project()), history[cursor], "annuler");
            // L'automate relit l'état du fichier d'origine (déclaration et marge).
            let p = s.doc.project().piece(s.piece).unwrap();
            net = p.net_cut;
            seam = p.seam.as_ref().map(|x| x.default_mm);
            // Le polygone suit le projet (annuler un déplacement).
            s.v = read_polygon(&s);
        } else if op == 8 && cursor + 1 < history.len() {
            assert!(s.doc.redo(), "rétablissement possible");
            cursor += 1;
            c.redos += 1;
            assert_eq!(bytes(s.doc.project()), history[cursor], "rétablir");
            let p = s.doc.project().piece(s.piece).unwrap();
            net = p.net_cut;
            seam = p.seam.as_ref().map(|x| x.default_mm);
            s.v = read_polygon(&s);
        }
        // Plus d'annulation que d'étapes : jamais.
        assert_eq!(s.doc.can_undo(), cursor > 0, "pile d'annulation");
        check_state(&s, net, seam, c);
    }
    trace.push(bytes(s.doc.project()));
    trace
}

/// Sommets du polygone relus dans le projet (pour suivre annuler / rétablir) : le début de chaque
/// tracé du contour, dans l'ordre de la pièce.
fn read_polygon(s: &Scene) -> Vec<PointMm> {
    if s.v.is_empty() {
        return Vec::new();
    }
    let p = s.doc.project();
    let sheet = p.piece_sheet(s.piece).unwrap();
    p.piece(s.piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| {
            let (a, b) = sheet.segment_ends(e.entity).unwrap();
            if e.reversed {
                b
            } else {
                a
            }
        })
        .collect()
}

#[test]
fn campagne_adverse_coupe_nette_160_sequences() {
    let mut c = Counters::default();
    let mut digest = Vec::new();
    for index in 0..160 {
        let result = catch_unwind(AssertUnwindSafe(|| run_sequence(index, &mut c)));
        match result {
            Ok(t) => digest.push(t),
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
    // Les compteurs prouvent que les contrôles forts ont tourné.
    eprintln!(
        "séquences {} étapes {} déclarations {} retraits {} refus coupe/marge {} refus marge/coupe {} \
         marges {} no-op {} déplacements {} annuler {} rétablir {} coupes {} relectures {} disques {} \
         inconnues {}",
        c.sequences,
        c.steps,
        c.declared,
        c.undeclared,
        c.refused_net_on_seam,
        c.refused_seam_on_net,
        c.seams_set,
        c.noops,
        c.moves,
        c.undos,
        c.redos,
        c.strong_cuts,
        c.roundtrips,
        c.circles,
        c.unknown_piece
    );
    assert_eq!(c.sequences, 160);
    assert!(c.declared >= 60, "déclarations : {}", c.declared);
    assert!(c.undeclared >= 15, "retraits : {}", c.undeclared);
    assert!(
        c.refused_net_on_seam >= 20,
        "refus coupe/marge : {}",
        c.refused_net_on_seam
    );
    assert!(
        c.refused_seam_on_net >= 20,
        "refus marge/coupe : {}",
        c.refused_seam_on_net
    );
    assert!(c.seams_set >= 100, "marges : {}", c.seams_set);
    assert!(c.noops >= 20, "no-op : {}", c.noops);
    assert!(c.moves >= 50, "déplacements : {}", c.moves);
    assert!(
        c.undos >= 30 && c.redos >= 5,
        "annuler {} rétablir {}",
        c.undos,
        c.redos
    );
    assert!(
        c.strong_cuts >= 800,
        "contrôles de coupe : {}",
        c.strong_cuts
    );
    assert!(c.roundtrips >= 1000, "relectures : {}", c.roundtrips);
    assert!(c.circles >= 30, "disques : {}", c.circles);
    assert!(
        c.unknown_piece >= 20,
        "pièces inconnues : {}",
        c.unknown_piece
    );
    // Déterminisme : mêmes graines, mêmes résultats (octets finaux compris).
    let mut c2 = Counters::default();
    let again: Vec<Vec<String>> = (0..160).map(|i| run_sequence(i, &mut c2)).collect();
    assert_eq!(digest, again, "déterminisme");
}
