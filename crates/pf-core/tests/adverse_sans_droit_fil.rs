//! PH-5d — campagne adverse « sans droit-fil » : 170 séquences à graine fixe de 2 à 4 rectangles,
//! déclarations (vraies, fausses, redondantes, pièce inconnue), droit-fil posés (y compris sur une
//! pièce déclarée : refus), retirés ou supprimés avec leur tracé, modifications de fiche, copies
//! de feuille, séparations, annulations et rétablissements exacts à l'octet, relectures JSON
//! (champ ajouté sur une pièce avec ou sans droit-fil, `false` explicite).
//!
//! **Oracle indépendant** : un miroir (`Entry` : droit-fil oui / non, déclaration oui / non) est
//! mis à jour par la **règle métier** écrite ici (une pièce avertit si et seulement si elle n'a ni
//! droit-fil ni déclaration ; les deux s'excluent ; une copie reprend les deux ; une moitié reprend
//! la déclaration) et comparé après chaque pas : champs bruts du modèle, `piece_issues`, rapport de
//! contrôle, recomptage des pièces sans droit-fil ni déclaration. Des **compteurs** prouvent que les
//! contrôles forts s'exécutent. Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 83).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::PointMm;

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
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    steps: usize,
    declared: usize,
    removed: usize,
    redundant: usize,
    refused_has_grain: usize,
    refused_unknown: usize,
    refused_set_grain: usize,
    grains_set: usize,
    grains_removed: usize,
    grains_deleted: usize,
    cards: usize,
    copies: usize,
    splits: usize,
    undo_checks: usize,
    redo_checks: usize,
    json_checks: usize,
    json_refused: usize,
    state_checks: usize,
    warned_seen: usize,
    unwarned_declared_seen: usize,
}

/// État attendu d'une pièce, d'après la règle métier seule.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    id: PieceId,
    has_grain: bool,
    no_grain: bool,
    /// Abscisse du rectangle d'origine (séparable) ; `None` pour une moitié ou une copie d'une moitié.
    x: Option<i64>,
}

struct Seq {
    doc: Document,
    mirror: Vec<Entry>,
    /// Pas d'historique : (miroir d'avant, octets d'avant).
    hist: Vec<(Vec<Entry>, String)>,
    /// Pas rétablissables : (miroir d'après, octets d'après).
    redo: Vec<(Vec<Entry>, String)>,
    next_comment: u32,
    c: Counters,
}

/// Résultat attendu d'une commande.
enum Expect {
    /// Réussit et ouvre un pas d'historique ; miroir d'après.
    Step(Vec<Entry>),
    /// Réussit sans aucune étape (octets identiques).
    NoOp,
    /// Refusée sans trace, message contenant la chaîne.
    Refuse(&'static str),
}

impl Seq {
    fn new(rng: &mut Rng) -> Seq {
        let mut p = Project::new("Campagne");
        let s = p.sheets[0].id;
        let n = 2 + rng.below(3) as usize;
        let mut mirror = Vec::new();
        for i in 0..n {
            let x = 300 * i as i64;
            let r = p
                .apply(Command::AddRectangle {
                    sheet: s,
                    corner: PointRef::New(pt(x as f64, 0.0)),
                    width_mm: 100.0,
                    height_mm: 60.0,
                })
                .unwrap();
            let id = p
                .apply(Command::ExtractPiece {
                    sheet: s,
                    source: PieceSource::Paths {
                        entities: r.entities,
                    },
                    name: format!("P{i}"),
                    detached: false,
                })
                .unwrap()
                .pieces[0];
            mirror.push(Entry {
                id,
                has_grain: false,
                no_grain: false,
                x: Some(x),
            });
        }
        Seq {
            doc: Document::new(p),
            mirror,
            hist: Vec::new(),
            redo: Vec::new(),
            next_comment: 0,
            c: Counters::default(),
        }
    }

    fn project(&self) -> &Project {
        self.doc.project()
    }

    fn run(&mut self, cmd: Command, expect: Expect) -> Option<Outcome> {
        let before = bytes(self.project());
        let before_p = self.project().clone();
        self.c.steps += 1;
        match expect {
            Expect::Step(next) => {
                let out = self.doc.execute(cmd).expect("la commande doit réussir");
                assert_ne!(bytes(self.project()), before, "un pas attendu");
                self.hist.push((self.mirror.clone(), before));
                self.redo.clear();
                self.mirror = next;
                Some(out)
            }
            Expect::NoOp => {
                let out = self.doc.execute(cmd).expect("le sans-effet doit réussir");
                assert_eq!(bytes(self.project()), before, "aucune étape attendue");
                assert_eq!(*self.project(), before_p);
                Some(out)
            }
            Expect::Refuse(word) => {
                let e = self.doc.execute(cmd).expect_err("refus attendu");
                assert!(e.to_string().contains(word), "« {e} » sans « {word} »");
                assert_eq!(*self.project(), before_p, "refus sans trace (valeur)");
                assert_eq!(bytes(self.project()), before, "refus sans trace (octets)");
                None
            }
        }
    }

    /// Contrôles de l'oracle : champs bruts, avertissements, rapport, recomptage.
    fn check(&mut self) {
        let p = self.doc.project();
        assert_eq!(p.pieces.len(), self.mirror.len(), "nombre de pièces");
        let (mut expected_warned, mut quiet_declared) = (0, 0);
        for e in &self.mirror {
            let piece = p.piece(e.id).expect("pièce du miroir");
            assert_eq!(piece.no_grain, e.no_grain, "déclaration de {}", piece.name);
            assert_eq!(
                piece.grainline.is_some(),
                e.has_grain,
                "droit-fil de {}",
                piece.name
            );
            assert!(
                !(piece.no_grain && (piece.grainline.is_some() || piece.grain.is_some())),
                "exclusion : {}",
                piece.name
            );
            let warns = p.piece_issues(e.id).contains(&PieceIssue::MissingGrainline);
            assert_eq!(warns, !e.has_grain && !e.no_grain, "{}", piece.name);
            let card = p.piece_card(e.id).unwrap();
            assert_eq!(card.no_grain, e.no_grain);
            assert_eq!(card.has_grainline, e.has_grain);
            if warns {
                expected_warned += 1;
            }
            if e.no_grain && !warns {
                quiet_declared += 1;
            }
        }
        // Recomptage direct sur le modèle brut, et dans le rapport de contrôle.
        let recount = p
            .pieces
            .iter()
            .filter(|q| q.grainline.is_none() && !q.no_grain)
            .count();
        assert_eq!(recount, expected_warned);
        let report = p.control_report();
        let anomalies = report
            .anomalies
            .iter()
            .filter(|a| a.kind == control_report::AnomalyKind::PieceIssue)
            .count();
        assert_eq!(anomalies, expected_warned, "anomalies du rapport");
        let issues: usize = report
            .pieces
            .iter()
            .map(|q| {
                q.issues
                    .iter()
                    .filter(|i| **i == PieceIssue::MissingGrainline)
                    .count()
            })
            .sum();
        assert_eq!(issues, expected_warned);
        self.c.warned_seen += expected_warned;
        self.c.unwarned_declared_seen += quiet_declared;
        self.c.state_checks += 1;
    }

    fn pick(&self, rng: &mut Rng) -> usize {
        rng.below(self.mirror.len() as u64) as usize
    }

    fn with(&self, idx: usize, f: impl Fn(&mut Entry)) -> Vec<Entry> {
        let mut m = self.mirror.clone();
        f(&mut m[idx]);
        m
    }

    fn sheet_of(&self, id: PieceId) -> SheetId {
        self.project().piece_sheet(id).unwrap().id
    }

    // ----- opérations ---------------------------------------------------------------------

    fn declare(&mut self, rng: &mut Rng, on: bool) {
        if rng.chance(6) {
            self.run(
                Command::SetPieceNoGrain {
                    piece: PieceId(900_000 + rng.below(100)),
                    no_grain: on,
                },
                Expect::Refuse(""),
            );
            self.c.refused_unknown += 1;
            return;
        }
        let i = self.pick(rng);
        let e = self.mirror[i].clone();
        let cmd = Command::SetPieceNoGrain {
            piece: e.id,
            no_grain: on,
        };
        if on == e.no_grain {
            self.run(cmd, Expect::NoOp);
            self.c.redundant += 1;
        } else if on && e.has_grain {
            self.run(cmd, Expect::Refuse("a un droit-fil"));
            self.c.refused_has_grain += 1;
        } else {
            let next = self.with(i, |x| x.no_grain = on);
            self.run(cmd, Expect::Step(next));
            if on {
                self.c.declared += 1;
            } else {
                self.c.removed += 1;
            }
        }
    }

    /// Crée un segment de rôle droit-fil (deux pas qui ne changent pas le miroir).
    fn new_grain_segment(&mut self, id: PieceId, rng: &mut Rng) -> EntityId {
        let sheet = self.sheet_of(id);
        let x = rng.below(80) as f64 + 5.0;
        let mirror = self.mirror.clone();
        let g = self
            .run(
                Command::AddSegment {
                    sheet,
                    from: PointRef::New(pt(x, 5.0)),
                    to: PointRef::New(pt(x, 15.0 + rng.below(20) as f64)),
                },
                Expect::Step(mirror.clone()),
            )
            .unwrap()
            .entities[0];
        self.run(
            Command::SetPathRole {
                sheet,
                entity: g,
                role: PathRole::Grainline,
            },
            Expect::Step(mirror),
        );
        g
    }

    fn set_grain(&mut self, rng: &mut Rng) {
        let i = self.pick(rng);
        let e = self.mirror[i].clone();
        let g = self.new_grain_segment(e.id, rng);
        let outline = self.project().piece(e.id).unwrap().outline.clone();
        let cmd = Command::SetPieceOutline {
            piece: e.id,
            outline,
            grainline: Some(g),
        };
        if e.no_grain {
            self.run(cmd, Expect::Refuse("sans droit-fil"));
            self.c.refused_set_grain += 1;
        } else {
            let next = self.with(i, |x| x.has_grain = true);
            self.run(cmd, Expect::Step(next));
            self.c.grains_set += 1;
        }
    }

    fn remove_grain(&mut self, rng: &mut Rng) {
        let i = self.pick(rng);
        let e = self.mirror[i].clone();
        if !e.has_grain {
            return;
        }
        let outline = self.project().piece(e.id).unwrap().outline.clone();
        let next = self.with(i, |x| x.has_grain = false);
        self.run(
            Command::SetPieceOutline {
                piece: e.id,
                outline,
                grainline: None,
            },
            Expect::Step(next),
        );
        self.c.grains_removed += 1;
    }

    fn delete_grain(&mut self, rng: &mut Rng) {
        let i = self.pick(rng);
        let e = self.mirror[i].clone();
        if !e.has_grain {
            return;
        }
        let g = self.project().piece(e.id).unwrap().grainline.unwrap();
        let sheet = self.sheet_of(e.id);
        let next = self.with(i, |x| x.has_grain = false);
        self.run(
            Command::Delete {
                sheet,
                points: Vec::new(),
                entities: vec![g],
            },
            Expect::Step(next),
        );
        self.c.grains_deleted += 1;
    }

    /// Modification de fiche : ne touche ni déclaration ni droit-fil.
    fn card(&mut self, rng: &mut Rng) {
        let i = self.pick(rng);
        let id = self.mirror[i].id;
        let piece = self.project().piece(id).unwrap().clone();
        self.next_comment += 1;
        let next = self.mirror.clone();
        self.run(
            Command::SetPieceCard {
                piece: id,
                name: piece.name,
                code: piece.code,
                quantity: piece.quantity,
                mirrored_pair: piece.mirrored_pair,
                material: Some("Cuir".into()),
                category: Some(PieceCategory::Leather),
                comment: Some(format!("c{}", self.next_comment)),
            },
            Expect::Step(next),
        );
        self.c.cards += 1;
    }

    fn duplicate(&mut self, rng: &mut Rng) {
        if self.mirror.len() >= 9 {
            return;
        }
        let i = self.pick(rng);
        let sheet = self.sheet_of(self.mirror[i].id);
        let originals: Vec<PieceId> = self
            .project()
            .pieces
            .iter()
            .filter(|q| matches!(q.link, PieceLink::Linked { sheet: s } if s == sheet))
            .map(|q| q.id)
            .collect();
        let mut next = self.mirror.clone();
        // Le miroir des copies est écrit d'avance : mêmes états que les originaux, même ordre.
        let copies: Vec<Entry> = originals
            .iter()
            .map(|o| {
                let m = self.mirror.iter().find(|e| e.id == *o).unwrap().clone();
                Entry {
                    id: PieceId(0),
                    ..m
                }
            })
            .collect();
        let out = self
            .run(
                Command::DuplicateSheet { sheet },
                Expect::Step(self.mirror.clone()),
            )
            .unwrap();
        let new_sheet = out.sheet.unwrap();
        let new_ids: Vec<PieceId> = self
            .project()
            .pieces
            .iter()
            .filter(|q| matches!(q.link, PieceLink::Linked { sheet: s } if s == new_sheet))
            .map(|q| q.id)
            .collect();
        assert_eq!(new_ids.len(), copies.len(), "une copie par pièce liée");
        for (id, mut m) in new_ids.into_iter().zip(copies) {
            m.id = id;
            next.push(m);
        }
        // Le pas a déjà été ouvert avec l'ancien miroir : le miroir d'après est complété.
        self.mirror = next;
        self.c.copies += 1;
    }

    fn split(&mut self, rng: &mut Rng) {
        let i = self.pick(rng);
        let e = self.mirror[i].clone();
        let Some(x) = e.x else { return };
        if e.has_grain || self.mirror.len() >= 9 {
            return;
        }
        let cmd = Command::SplitPiece {
            piece: e.id,
            a: pt(x as f64 + 50.0, -10.0),
            b: pt(x as f64 + 50.0, 70.0),
        };
        // Les deux moitiés reprennent la déclaration et n'ont pas de droit-fil.
        let mut next: Vec<Entry> = self
            .mirror
            .iter()
            .filter(|m| m.id != e.id)
            .cloned()
            .collect();
        let before = bytes(self.project());
        let out = self.doc.execute(cmd).expect("séparation");
        self.c.steps += 1;
        assert_ne!(bytes(self.project()), before);
        assert_eq!(out.pieces.len(), 2);
        for id in out.pieces {
            next.push(Entry {
                id,
                has_grain: false,
                no_grain: e.no_grain,
                x: None,
            });
        }
        self.hist.push((self.mirror.clone(), before));
        self.redo.clear();
        self.mirror = next;
        self.c.splits += 1;
    }

    fn undo(&mut self) {
        let Some((old_mirror, old_bytes)) = self.hist.pop() else {
            assert!(!self.doc.can_undo());
            return;
        };
        let after = bytes(self.project());
        assert!(self.doc.undo());
        assert_eq!(bytes(self.project()), old_bytes, "annuler = octets d'avant");
        self.redo.push((self.mirror.clone(), after));
        self.mirror = old_mirror;
        self.c.undo_checks += 1;
    }

    fn redo(&mut self) {
        let Some((new_mirror, new_bytes)) = self.redo.pop() else {
            assert!(!self.doc.can_redo());
            return;
        };
        let before = bytes(self.project());
        assert!(self.doc.redo());
        assert_eq!(
            bytes(self.project()),
            new_bytes,
            "rétablir = octets d'après"
        );
        self.hist.push((self.mirror.clone(), before));
        self.mirror = new_mirror;
        self.c.redo_checks += 1;
    }

    fn json(&mut self, rng: &mut Rng) {
        let text = bytes(self.project());
        // Relecture : mêmes octets, mêmes états.
        let back = persist::from_json(&text).expect("relecture");
        assert_eq!(bytes(&back), text, "aller-retour exact");
        for e in &self.mirror {
            let q = back.piece(e.id).unwrap();
            assert_eq!(
                (q.no_grain, q.grainline.is_some()),
                (e.no_grain, e.has_grain)
            );
        }
        // Le champ n'est écrit que pour les pièces déclarées.
        let written = text.matches("\"no_grain\": true").count();
        assert_eq!(written, self.mirror.iter().filter(|e| e.no_grain).count());
        assert_eq!(text.matches("no_grain").count(), written, "jamais `false`");
        // Altération : `true` ajouté sur une pièce, `false` explicite.
        let i = self.pick(rng);
        let e = self.mirror[i].clone();
        let tamper = |value: serde_json::Value| {
            let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
            let at = v["pieces"]
                .as_array()
                .unwrap()
                .iter()
                .position(|q| q["id"] == serde_json::json!(e.id.0))
                .unwrap();
            v["pieces"][at]["no_grain"] = value;
            serde_json::to_string_pretty(&v).unwrap()
        };
        let res = persist::from_json(&tamper(serde_json::json!(true)));
        if e.has_grain {
            assert!(res.is_err(), "droit-fil et déclaration cumulés");
            self.c.json_refused += 1;
        } else {
            assert!(res.unwrap().piece(e.id).unwrap().no_grain);
        }
        assert!(persist::from_json(&tamper(serde_json::json!(false))).is_err());
        self.c.json_refused += 1;
        self.c.json_checks += 1;
    }
}

#[test]
fn campagne_sans_droit_fil_170_sequences() {
    let mut total = Counters::default();
    for k in 0..170u64 {
        let mut rng = Rng(0x5D5D_0000 + k * 7919 + 1);
        let mut s = Seq::new(&mut rng);
        s.check();
        for _ in 0..28 {
            match rng.below(100) {
                0..=19 => s.declare(&mut rng, true),
                20..=29 => s.declare(&mut rng, false),
                30..=42 => s.set_grain(&mut rng),
                43..=47 => s.remove_grain(&mut rng),
                48..=51 => s.delete_grain(&mut rng),
                52..=57 => s.card(&mut rng),
                58..=65 => s.duplicate(&mut rng),
                66..=72 => s.split(&mut rng),
                73..=82 => s.undo(),
                83..=90 => s.redo(),
                _ => s.json(&mut rng),
            }
            s.check();
        }
        // Tout annuler ramène au projet de départ ; tout rétablir revient à l'état final.
        while s.doc.can_redo() {
            s.redo();
            s.check();
        }
        let last = bytes(s.project());
        let mut depth = 0;
        while s.doc.can_undo() {
            s.undo();
            depth += 1;
            s.check();
        }
        assert_eq!(depth, s.redo.len());
        for _ in 0..depth {
            s.redo();
            s.check();
        }
        assert_eq!(bytes(s.project()), last, "séquence {k}");
        let c = &s.c;
        total.sequences += 1;
        total.steps += c.steps;
        total.declared += c.declared;
        total.removed += c.removed;
        total.redundant += c.redundant;
        total.refused_has_grain += c.refused_has_grain;
        total.refused_unknown += c.refused_unknown;
        total.refused_set_grain += c.refused_set_grain;
        total.grains_set += c.grains_set;
        total.grains_removed += c.grains_removed;
        total.grains_deleted += c.grains_deleted;
        total.cards += c.cards;
        total.copies += c.copies;
        total.splits += c.splits;
        total.undo_checks += c.undo_checks;
        total.redo_checks += c.redo_checks;
        total.json_checks += c.json_checks;
        total.json_refused += c.json_refused;
        total.state_checks += c.state_checks;
        total.warned_seen += c.warned_seen;
        total.unwarned_declared_seen += c.unwarned_declared_seen;
    }
    // Les contrôles forts ont bien été exécutés (jamais une campagne qui ne teste rien).
    assert_eq!(total.sequences, 170);
    assert!(total.declared >= 300, "{total:?}");
    assert!(total.removed >= 100, "{total:?}");
    assert!(total.redundant >= 100, "{total:?}");
    assert!(total.refused_has_grain >= 50, "{total:?}");
    assert!(total.refused_unknown >= 50, "{total:?}");
    assert!(total.refused_set_grain >= 50, "{total:?}");
    assert!(total.grains_set >= 200, "{total:?}");
    assert!(total.grains_removed >= 40, "{total:?}");
    assert!(total.grains_deleted >= 20, "{total:?}");
    assert!(total.cards >= 200, "{total:?}");
    assert!(total.copies >= 200, "{total:?}");
    assert!(total.splits >= 100, "{total:?}");
    assert!(total.undo_checks >= 500, "{total:?}");
    assert!(total.redo_checks >= 300, "{total:?}");
    assert!(total.json_checks >= 150, "{total:?}");
    assert!(total.json_refused >= 300, "{total:?}");
    assert!(total.warned_seen >= 2000, "{total:?}");
    assert!(total.unwarned_declared_seen >= 2000, "{total:?}");
    eprintln!("{total:?}");
}
