//! Support commun aux tests de la bibliothèque (US-11.4) : projet source, dossier temporaire,
//! collecte d'identifiants lue **dans le JSON** (jamais par le code testé).

#![allow(dead_code)]

use pf_core::persist;
use pf_core::*;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Dossier temporaire unique, supprimé à la fin.
pub struct Tmp(pub PathBuf);

impl Tmp {
    pub fn new(tag: &str) -> Tmp {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pf-lib-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Tmp(dir)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn file(&self, id: u64) -> PathBuf {
        self.0.join(format!("lib-{id:06}.pforge-lib.json"))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

/// Projet source : la pièce « Rabat » de la démonstration (6 points, 5 tracés, 3 crans,
/// 2 cotations, cuir, quantité 2), une variable `L` = `l`, et un point sur le premier côté à la
/// distance `L` (formule maintenue), repère interne de la pièce.
pub struct Fx {
    pub p: Project,
    pub sheet: SheetId,
    pub piece: PieceId,
    pub base: EntityId,
    pub point: PointId,
}

pub fn fixture(l: &str) -> Fx {
    let mut p = demo_piece_project().unwrap();
    p.apply(Command::AddVariable {
        name: "L".into(),
        unit: VariableUnit::Mm,
        expr: l.into(),
    })
    .unwrap();
    let sheet = p.sheets[0].id;
    let piece = p.pieces[0].id;
    let base = p.sheets[0].entities[0].id;
    let out = p
        .apply(Command::AddPointWithFormulas {
            sheet,
            definition: PointDefinition::OnSegment {
                segment: base,
                distance_mm: 1.0,
                origin: SegmentEnd::Start,
            },
            formulas: vec![ParamExpr {
                param: ParamKind::SegmentDistance,
                expr: "L".into(),
            }],
        })
        .unwrap();
    let point = out.points[0];
    p.apply(Command::AddPieceInternal {
        piece,
        target: InternalTarget::Point { point },
        kind: InternalKind::Mark,
    })
    .unwrap();
    Fx {
        p,
        sheet,
        piece,
        base,
        point,
    }
}

/// Projet cible vide, avec ou sans variable `L`.
pub fn target(l: Option<&str>) -> Project {
    let mut p = Project::new("Cible");
    if let Some(l) = l {
        p.apply(Command::AddVariable {
            name: "L".into(),
            unit: VariableUnit::Mm,
            expr: l.into(),
        })
        .unwrap();
    }
    p
}

/// Tous les nombres portés par une clé `id` dans un JSON (feuilles, objets, pièces et leurs
/// éléments).
pub fn collect_ids(v: &Value, out: &mut BTreeSet<u64>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "id" {
                    if let Some(n) = x.as_u64() {
                        out.insert(n);
                    }
                }
                collect_ids(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_ids(x, out)),
        _ => {}
    }
}

/// Identifiants d'un ensemble de feuilles et de pièces d'un projet.
pub fn project_ids(p: &Project, sheets: &[SheetId]) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for s in sheets {
        collect_ids(
            &serde_json::to_value(p.sheet(*s).unwrap()).unwrap(),
            &mut ids,
        );
    }
    for pc in &p.pieces {
        if let PieceLink::Linked { sheet } = pc.link {
            if sheets.contains(&sheet) {
                collect_ids(&serde_json::to_value(pc).unwrap(), &mut ids);
            }
        }
    }
    ids
}

pub fn pos(p: &Project, s: SheetId, pt: PointId) -> (f64, f64) {
    let q = p.sheet(s).unwrap().position(pt).unwrap();
    (q.x, q.y)
}

pub fn insert(content: &LibraryContent) -> Command {
    Command::InsertLibrary {
        content: Box::new(content.clone()),
        sheet: None,
        layer: None,
        rename_variables: false,
    }
}
