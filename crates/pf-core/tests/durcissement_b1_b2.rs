//! Durcissement 1, B1 et B2 : les optimisations ne changent **rien** à l'état.
//!
//! - B2 : `Document::execute` est comparé à un modèle de référence qui garde l'ancienne façon de
//!   faire (copie explicite avant, `Project::apply`, comparaison) : projet identique à l'octet,
//!   refus sans trace (y compris au milieu d'un geste), annulation d'un geste, une étape par
//!   commande hors geste et une seule par geste.
//! - B1 : le point réutilisé à la création est celui que désigne l'oracle de résolution (premier
//!   point de la feuille à 1e-6 mm au plus de la position visée).
//!
//! L'empreinte affichée avec `--nocapture` a été comparée entre le code d'avant et d'après.

mod adverse_support;
mod durcissement_oracle;

use adverse_support::*;
use durcissement_oracle::o_pos;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const SEQUENCES: u64 = 160;
const STEPS: usize = 40;

/// Référence : l'ancien `Document`, avec ses deux copies.
struct Model {
    project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    group_start: Option<Project>,
}

impl Model {
    fn push(&mut self, state: Project) {
        self.undo.push(state);
        self.redo.clear();
    }

    fn execute(&mut self, command: Command) -> Result<Outcome, CoreError> {
        let before = self.project.clone();
        let outcome = self.project.apply(command);
        if outcome.is_err() {
            assert!(self.project == before, "un refus a laissé une trace");
        }
        let outcome = outcome?;
        if self.group_start.is_none() && self.project != before {
            self.push(before);
        }
        Ok(outcome)
    }

    fn begin(&mut self) {
        if self.group_start.is_none() {
            self.group_start = Some(self.project.clone());
        }
    }

    fn end(&mut self) {
        if let Some(start) = self.group_start.take() {
            if start != self.project {
                self.push(start);
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(start) = self.group_start.take() {
            self.project = start;
        }
    }

    fn undo(&mut self) -> bool {
        self.end();
        match self.undo.pop() {
            Some(p) => {
                self.redo.push(std::mem::replace(&mut self.project, p));
                true
            }
            None => false,
        }
    }

    fn redo(&mut self) -> bool {
        self.end();
        match self.redo.pop() {
            Some(p) => {
                self.undo.push(std::mem::replace(&mut self.project, p));
                true
            }
            None => false,
        }
    }
}

fn context() -> Ctx {
    Ctx {
        donors: donors(true, true),
        bank: Vec::new(),
        arcs: true,
        splines: true,
        tangency: true,
        traces: true,
        relative: true,
        divisions: true,
        paths: true,
        guides: true,
        offsets: true,
        mirrors: true,
    }
}

/// Commande de création de point visant exactement (ou presque) un point existant, ou ailleurs.
fn creation(r: &mut Rng, p: &Project) -> Option<(Command, SheetId, PointDefinition)> {
    let sheet = &p.sheets[r.below(p.sheets.len())];
    if sheet.points.is_empty() {
        return None;
    }
    let pick = |r: &mut Rng| sheet.points[r.below(sheet.points.len())].id;
    let near = |r: &mut Rng| [0.0, 4.0e-7, 2.0e-6, 3.0][r.below(4)];
    let (id, other) = (pick(r), pick(r));
    let definition = match r.below(4) {
        0 => {
            let at = o_pos(sheet, id, &mut Vec::new()).ok()?;
            PointDefinition::Free {
                x: at.x + near(r),
                y: at.y,
            }
        }
        1 => PointDefinition::Offset {
            from: id,
            dx_mm: near(r),
            dy_mm: 0.0,
        },
        2 => PointDefinition::Midpoint {
            first: id,
            second: other,
        },
        _ => PointDefinition::Offset {
            from: id,
            dx_mm: r.below(40) as f64,
            dy_mm: r.below(40) as f64,
        },
    };
    let sheet_id = sheet.id;
    let command = match definition {
        PointDefinition::Free { x, y } if r.chance(50) => Command::AddPoint {
            sheet: sheet_id,
            at: PointMm::new(x, y),
        },
        PointDefinition::Free { x, y } => Command::AddPointAt {
            sheet: sheet_id,
            point: PointRef::New(PointMm::new(x, y)),
        },
        d => Command::AddPointAt {
            sheet: sheet_id,
            point: PointRef::Derived(d),
        },
    };
    Some((command, sheet_id, definition))
}

/// Oracle de B1 : le point qu'une création doit réutiliser.
fn expected_reuse(sheet: &Sheet, definition: PointDefinition) -> Option<Option<PointId>> {
    // La position visée est celle d'un point de même définition ajouté à une copie.
    let mut probe = sheet.clone();
    probe.points.push(Point {
        id: PointId(u64::MAX),
        label: "nouveau".into(),
        definition,
        role: PointRole::Construction,
        layer: None,
    });
    let at = o_pos(&probe, PointId(u64::MAX), &mut Vec::new()).ok()?;
    Some(sheet.points.iter().find_map(|p| {
        let pos = o_pos(sheet, p.id, &mut Vec::new()).ok()?;
        (distance_mm(pos, at) <= 1e-6).then_some(p.id)
    }))
}

fn bytes(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

#[derive(Default)]
struct Stats {
    accepted: usize,
    refused: usize,
    refused_in_group: usize,
    cancelled: usize,
    grouped: usize,
    reused: usize,
    created: usize,
}

fn sequence(seed: u64, stats: &mut Stats, digest: &mut DefaultHasher) {
    let mut r = Rng(0xB1B2_0000 + seed);
    let mut ctx = context();
    let start = match seed % 4 {
        0 => rich_project(),
        1 => rich_project_with_arcs(),
        2 => rich_project_with_splines(),
        _ => pf_core::demo_piece_project().unwrap(),
    };
    let mut doc = Document::new(start.clone());
    let mut model = Model {
        project: start,
        undo: Vec::new(),
        redo: Vec::new(),
        group_start: None,
    };
    let mut grouping = false;
    for step in 0..STEPS {
        let label = format!("graine {seed}, pas {step}");
        match r.below(100) {
            0..=6 => {
                doc.begin_group();
                model.begin();
                grouping = true;
            }
            7..=11 => {
                doc.end_group();
                model.end();
                stats.grouped += usize::from(grouping);
                grouping = false;
            }
            12..=15 => {
                doc.cancel_group();
                model.cancel();
                stats.cancelled += usize::from(grouping);
                grouping = false;
            }
            16..=20 => {
                assert_eq!(doc.undo(), model.undo(), "{label}");
                grouping = false;
            }
            21..=23 => {
                assert_eq!(doc.redo(), model.redo(), "{label}");
                grouping = false;
            }
            n => {
                let creation = (n < 45).then(|| creation(&mut r, doc.project())).flatten();
                let (command, expected) = match creation {
                    Some((command, sheet, definition)) => {
                        let s = doc.project().sheet(sheet).unwrap();
                        let expected =
                            expected_reuse(s, definition).map(|e| (sheet, e, s.points.len()));
                        (command, expected)
                    }
                    None => (gen(&mut r, doc.project(), &ctx, 0), None),
                };
                let before = bytes(doc.project());
                let got = doc.execute(command.clone());
                let reference = model.execute(command.clone());
                assert_eq!(got, reference, "{label} : {command:?}");
                match got {
                    Ok(outcome) => {
                        stats.accepted += 1;
                        if let Some((sheet, reuse, count)) = expected {
                            let after = doc.project().sheet(sheet).unwrap();
                            match reuse {
                                Some(_) => {
                                    assert!(outcome.points.is_empty(), "{label} : doublon");
                                    assert_eq!(after.points.len(), count, "{label}");
                                    stats.reused += 1;
                                }
                                None => {
                                    assert_eq!(outcome.points.len(), 1, "{label} : {command:?}");
                                    assert_eq!(after.points.len(), count + 1, "{label}");
                                    stats.created += 1;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        stats.refused += 1;
                        stats.refused_in_group += usize::from(grouping);
                        assert_eq!(bytes(doc.project()), before, "{label} : refus avec trace");
                    }
                }
            }
        }
        assert!(
            doc.project() == &model.project,
            "{label} : états différents"
        );
        assert_eq!(doc.can_undo(), !model.undo.is_empty(), "{label}");
        assert_eq!(doc.can_redo(), !model.redo.is_empty(), "{label}");
        bytes(doc.project()).hash(digest);
        if r.chance(6) {
            ctx.bank.extend(
                doc.project()
                    .copy_fragment_with_guides(
                        doc.project().sheets[0].id,
                        &doc.project().sheets[0]
                            .points
                            .iter()
                            .take(2)
                            .map(|p| p.id)
                            .collect::<Vec<_>>(),
                        &doc.project().sheets[0]
                            .entities
                            .iter()
                            .take(2)
                            .map(|e| e.id)
                            .collect::<Vec<_>>(),
                        &[],
                        &[],
                        &[],
                    )
                    .ok(),
            );
        }
    }
    // Toute l'histoire, état par état.
    loop {
        let (a, b) = (doc.undo(), model.undo());
        assert_eq!(a, b, "graine {seed} : remontée");
        assert_eq!(bytes(doc.project()), bytes(&model.project), "graine {seed}");
        if !a {
            break;
        }
    }
    while doc.redo() {
        assert!(model.redo());
        assert_eq!(bytes(doc.project()), bytes(&model.project), "graine {seed}");
    }
    assert!(!model.redo());
}

#[test]
fn sequences_identiques_au_modele_de_reference() {
    let mut stats = Stats::default();
    let mut digest = DefaultHasher::new();
    for seed in 0..SEQUENCES {
        sequence(seed, &mut stats, &mut digest);
    }
    eprintln!(
        "empreinte {:016x} | acceptées {} refusées {} (en geste {}) | gestes {} annulés {} | \
         points réutilisés {} créés {}",
        digest.finish(),
        stats.accepted,
        stats.refused,
        stats.refused_in_group,
        stats.grouped,
        stats.cancelled,
        stats.reused,
        stats.created
    );
    assert!(stats.accepted > 1000 && stats.refused > 500);
    assert!(stats.refused_in_group > 50, "{}", stats.refused_in_group);
    assert!(stats.grouped > 20 && stats.cancelled > 20);
    assert!(stats.reused > 100 && stats.created > 100);
}

/// Refus au milieu d'un geste, puis annulation du geste : cas écrit à la main.
#[test]
fn refus_au_milieu_d_un_geste_puis_annulation() {
    let mut doc = Document::new(Project::new("Geste"));
    let sheet = doc.project().sheets[0].id;
    let add = |x: f64| Command::AddPoint {
        sheet,
        at: PointMm::new(x, 0.0),
    };
    let a = doc.execute(add(0.0)).unwrap().points[0];
    let start = bytes(doc.project());
    doc.begin_group();
    doc.execute(add(10.0)).unwrap();
    let middle = bytes(doc.project());
    assert!(doc
        .execute(Command::MovePoint {
            sheet,
            point: PointId(999),
            to: PointMm::new(1.0, 1.0),
        })
        .is_err());
    assert_eq!(bytes(doc.project()), middle);
    doc.execute(Command::MovePoint {
        sheet,
        point: a,
        to: PointMm::new(5.0, 5.0),
    })
    .unwrap();
    doc.cancel_group();
    assert_eq!(bytes(doc.project()), start);
    // Le même geste, terminé : une seule étape.
    doc.begin_group();
    doc.execute(add(10.0)).unwrap();
    doc.execute(add(20.0)).unwrap();
    doc.end_group();
    assert_eq!(doc.project().sheets[0].points.len(), 3);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), start);
    assert!(doc.undo());
    assert!(doc.project().sheets[0].points.is_empty());
    assert!(!doc.undo());
}
