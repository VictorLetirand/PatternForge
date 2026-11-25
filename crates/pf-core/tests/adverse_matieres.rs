//! US-05.5 — campagne adverse des marges par matière : 160 séquences à graine fixe (jeux de
//! marges par catégorie valides et invalides, changements de catégorie, création de marge,
//! « appliquer la valeur de la catégorie », annuler / rétablir, aller-retour fichier).
//!
//! Oracle **indépendant** : un modèle à part (tableau des valeurs par catégorie, catégorie et marge
//! de chaque pièce, historique) avec les valeurs d'atelier écrites ici ; l'aire du contour de coupe
//! d'un rectangle est la formule `(l + 2m)(h + 2m)` ; aucune fonction de `piece_seam` ne juge le
//! résultat. Propriétés :
//!
//! 1. jamais de panique, déterminisme à graine égale ;
//! 2. valeur par catégorie du projet = oracle, après chaque commande ;
//! 3. marge et aire de chaque pièce = oracle (modifier le jeu ou la catégorie ne réécrit rien) ;
//! 4. refus (valeur hors bornes, pièce sans catégorie) : octets identiques ;
//! 5. no-op : aucune étape (le nombre d'annulations possibles égale celui du modèle) ;
//! 6. la clé `seam_defaults` est dans le fichier si et seulement si le jeu diffère des valeurs
//!    d'atelier ; aller-retour fichier exact ; annuler tout rend les octets de départ.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
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
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn q(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

const CATS: [PieceCategory; 5] = [
    PieceCategory::Fabric,
    PieceCategory::Lining,
    PieceCategory::Interfacing,
    PieceCategory::Leather,
    PieceCategory::Reinforcement,
];
/// Valeurs d'atelier, écrites ici (ordre de `CATS`).
const BUILTIN: [f64; 5] = [10.0, 10.0, 5.0, 6.0, 5.0];
const FALLBACK: f64 = 10.0;

#[derive(Clone, PartialEq, Debug)]
struct Model {
    eff: [f64; 5],
    cats: [Option<usize>; 3],
    margin: [f64; 3],
}

#[derive(Default)]
struct Counters {
    set_ok: usize,
    set_noop: usize,
    set_refused: usize,
    category: usize,
    created: usize,
    applied: usize,
    apply_refused: usize,
    undo: usize,
    redo: usize,
    roundtrip: usize,
    checked_areas: usize,
    keys_checked: usize,
}

struct Scene {
    doc: Document,
    pieces: [PieceId; 3],
    size: [(f64, f64); 3],
}

fn build(rng: &mut Rng) -> Scene {
    let mut p = Project::new("Adverse matières");
    let sheet = p.sheets[0].id;
    let mut pieces = Vec::new();
    let mut size = [(0.0, 0.0); 3];
    for (k, sz) in size.iter_mut().enumerate() {
        let (w, h) = (q(30.0 + 90.0 * rng.unit()), q(30.0 + 90.0 * rng.unit()));
        *sz = (w, h);
        let x0 = 2000.0 * k as f64;
        let v = [
            PointMm::new(x0, 0.0),
            PointMm::new(x0 + w, 0.0),
            PointMm::new(x0 + w, h),
            PointMm::new(x0, h),
        ];
        let ids: Vec<EntityId> = (0..4)
            .map(|i| {
                p.apply(Command::AddSegment {
                    sheet,
                    from: PointRef::New(v[i]),
                    to: PointRef::New(v[(i + 1) % 4]),
                })
                .unwrap()
                .entities[0]
            })
            .collect();
        pieces.push(
            p.apply(Command::ExtractPiece {
                sheet,
                source: PieceSource::Paths { entities: ids },
                name: format!("Pièce {k}"),
                detached: false,
            })
            .unwrap()
            .pieces[0],
        );
    }
    Scene {
        doc: Document::new(p),
        pieces: [pieces[0], pieces[1], pieces[2]],
        size,
    }
}

fn random_mm(rng: &mut Rng) -> f64 {
    match rng.below(7) {
        0 => 0.0,
        1 => 1000.0,
        2 => 5.0,
        3 => 6.0,
        4 => 10.0,
        _ => q(50.0 * rng.unit()),
    }
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

/// Compare le projet réel à l'oracle.
fn check(s: &Scene, m: &Model, c: &mut Counters) {
    let p = s.doc.project();
    p.validate().expect("projet valide");
    for (i, cat) in CATS.iter().enumerate() {
        assert_eq!(p.seam_default_for(Some(*cat)), m.eff[i], "{}", cat.key());
    }
    assert_eq!(p.seam_default_for(None), FALLBACK);
    for k in 0..3 {
        let piece = p.piece(s.pieces[k]).unwrap();
        assert_eq!(
            piece.category,
            m.cats[k].map(|i| CATS[i]),
            "catégorie de la pièce {k}"
        );
        let got = piece.seam.as_ref().map_or(0.0, |x| x.default_mm);
        assert_eq!(got, m.margin[k], "marge de la pièce {k}");
        let (w, h) = s.size[k];
        let want = (w + 2.0 * m.margin[k]) * (h + 2.0 * m.margin[k]);
        let area = p.piece_cut_contour(s.pieces[k]).unwrap().area_mm2;
        assert!(
            (area - want).abs() <= 1e-9 * want + 1e-6,
            "aire {area} ≠ {want}"
        );
        c.checked_areas += 1;
    }
    let text = bytes(p);
    assert_eq!(
        text.contains("seam_defaults"),
        m.eff != BUILTIN,
        "clé seam_defaults"
    );
    c.keys_checked += 1;
}

fn run_sequence(index: usize, c: &mut Counters) -> String {
    let mut rng = Rng(0xA5A5_0000 + index as u64);
    let mut s = build(&mut rng);
    let initial = bytes(s.doc.project());
    let mut m = Model {
        eff: BUILTIN,
        cats: [None; 3],
        margin: [0.0; 3],
    };
    let mut history: Vec<Model> = Vec::new();
    let mut redo: Vec<Model> = Vec::new();
    check(&s, &m, c);
    for _ in 0..22 {
        let k = rng.below(3);
        let piece = s.pieces[k];
        let before = m.clone();
        let before_bytes = bytes(s.doc.project());
        let mut changed = false;
        match rng.below(10) {
            0..=2 => {
                // Jeu valide : une ou plusieurs catégories changent (ou rien).
                let mut eff = m.eff;
                for v in eff.iter_mut() {
                    if rng.below(3) == 0 {
                        *v = random_mm(&mut rng);
                    }
                }
                let d = SeamDefaults {
                    fabric_mm: eff[0],
                    lining_mm: eff[1],
                    interfacing_mm: eff[2],
                    leather_mm: eff[3],
                    reinforcement_mm: eff[4],
                };
                s.doc
                    .execute(Command::SetSeamDefaults { defaults: Some(d) })
                    .unwrap();
                changed = eff != m.eff;
                if changed {
                    c.set_ok += 1;
                } else {
                    c.set_noop += 1;
                }
                m.eff = eff;
            }
            3 => {
                let bad = [-1.0, 1000.5, f64::NAN, f64::INFINITY, -1e-9][rng.below(5)];
                let mut d = SeamDefaults::builtin();
                d.set(CATS[rng.below(5)], bad);
                assert!(s
                    .doc
                    .execute(Command::SetSeamDefaults { defaults: Some(d) })
                    .is_err());
                c.set_refused += 1;
            }
            4 => {
                let cat = if rng.below(4) == 0 {
                    None
                } else {
                    Some(rng.below(5))
                };
                let card = s.doc.project().piece(piece).unwrap().clone();
                s.doc
                    .execute(Command::SetPieceCard {
                        piece,
                        name: card.name,
                        code: card.code,
                        quantity: card.quantity,
                        mirrored_pair: card.mirrored_pair,
                        material: card.material,
                        category: cat.map(|i| CATS[i]),
                        comment: card.comment,
                    })
                    .unwrap();
                changed = cat != m.cats[k];
                m.cats[k] = cat;
                c.category += 1;
            }
            5 | 6 => {
                // Création de la marge d'une pièce : valeur proposée = valeur de la catégorie, ou
                // le repli sans catégorie.
                let want = m.cats[k].map_or(FALLBACK, |i| m.eff[i]);
                let got = s.doc.project().seam_default_for(m.cats[k].map(|i| CATS[i]));
                assert_eq!(got, want, "valeur proposée");
                s.doc
                    .execute(Command::SetSeamAllowance {
                        piece,
                        seam: Some(SeamAllowance {
                            default_mm: got,
                            ..Default::default()
                        }),
                    })
                    .unwrap();
                changed = want != m.margin[k];
                m.margin[k] = want;
                c.created += 1;
            }
            7 | 8 => {
                // « Appliquer la valeur de la catégorie ».
                match m.cats[k] {
                    None => {
                        assert!(s.doc.project().category_seam_mm(piece).is_err());
                        c.apply_refused += 1;
                    }
                    Some(i) => {
                        let mm = s.doc.project().category_seam_mm(piece).unwrap();
                        assert_eq!(mm, m.eff[i]);
                        let mut next = s
                            .doc
                            .project()
                            .piece(piece)
                            .unwrap()
                            .seam
                            .clone()
                            .unwrap_or_default();
                        next.default_mm = mm;
                        s.doc
                            .execute(Command::SetSeamAllowance {
                                piece,
                                seam: Some(next),
                            })
                            .unwrap();
                        changed = m.eff[i] != m.margin[k];
                        m.margin[k] = m.eff[i];
                        c.applied += 1;
                    }
                }
            }
            _ => {
                if rng.below(2) == 0 {
                    if let Some(prev) = history.pop() {
                        assert!(s.doc.undo());
                        redo.push(m.clone());
                        m = prev;
                        c.undo += 1;
                    } else {
                        assert!(!s.doc.undo());
                    }
                } else if let Some(next) = redo.pop() {
                    assert!(s.doc.redo());
                    history.push(m.clone());
                    m = next;
                    c.redo += 1;
                } else {
                    assert!(!s.doc.redo());
                }
                check(&s, &m, c);
                continue;
            }
        }
        if changed {
            history.push(before);
            redo.clear();
        } else {
            assert_eq!(
                bytes(s.doc.project()),
                before_bytes,
                "refus ou no-op : octets identiques"
            );
        }
        check(&s, &m, c);
        // Aller-retour fichier.
        if rng.below(4) == 0 {
            let text = bytes(s.doc.project());
            let back = persist::from_json(&text).unwrap();
            assert_eq!(&back, s.doc.project());
            assert_eq!(bytes(&back), text);
            c.roundtrip += 1;
        }
    }
    // Annuler tout : autant d'étapes que le modèle, octets de départ.
    let mut steps = 0;
    while s.doc.undo() {
        steps += 1;
    }
    assert_eq!(steps, history.len(), "nombre d'étapes annulables");
    assert_eq!(bytes(s.doc.project()), initial);
    bytes(s.doc.project())
}

#[test]
fn campagne_adverse_matieres_160_sequences() {
    let mut c = Counters::default();
    let mut first = Vec::new();
    for i in 0..160 {
        first.push(
            catch_unwind(AssertUnwindSafe(|| run_sequence(i, &mut c)))
                .unwrap_or_else(|_| panic!("panique ou échec à la séquence {i}")),
        );
    }
    // Déterminisme : mêmes graines, mêmes octets de départ.
    let mut c2 = Counters::default();
    for (i, f) in first.iter().enumerate().take(20) {
        assert_eq!(&run_sequence(i, &mut c2), f);
    }
    // Les contrôles forts ont réellement tourné.
    assert!(c.set_ok >= 150, "jeux modifiés : {}", c.set_ok);
    assert!(c.set_noop >= 10, "no-op : {}", c.set_noop);
    assert!(c.set_refused >= 100, "refus : {}", c.set_refused);
    assert!(c.category >= 150, "catégories : {}", c.category);
    assert!(c.created >= 150, "créations : {}", c.created);
    assert!(c.applied >= 100, "applications : {}", c.applied);
    assert!(
        c.apply_refused >= 30,
        "refus sans catégorie : {}",
        c.apply_refused
    );
    assert!(
        c.undo >= 100 && c.redo >= 10,
        "undo {} redo {}",
        c.undo,
        c.redo
    );
    assert!(c.roundtrip >= 150, "aller-retour : {}", c.roundtrip);
    assert!(
        c.checked_areas >= 160 * 3 * 20,
        "aires : {}",
        c.checked_areas
    );
    assert!(c.keys_checked >= 160 * 20);
}
