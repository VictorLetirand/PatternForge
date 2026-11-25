//! Plans de coupe (socle d'E15) — campagne adverse : 170 séquences à graine fixe de 14 à 30 pas
//! sur un projet de 2 ou 3 pièces et d'une gamme de 4 tailles : création, mise à jour, suppression
//! de plans, pose / mise à jour / retrait de placements, suppression de pièces et de tailles
//! (refusée quand un placement les cite), annulations et rétablissements exacts à l'octet,
//! relectures JSON idempotentes. Chaque pas est une valeur valide ou **une seule** valeur
//! invalide, pour que le motif du refus soit exact.
//!
//! **Oracle indépendant** : un miroir (`Model`) tient plans, placements, pièces et tailles vivantes,
//! le compteur d'identifiants et les piles d'annulation. Il décide seul, d'après les règles du
//! cahier écrites ici (bornes, noms, lisières, références), si une commande est acceptée, refusée
//! (et pourquoi) ou sans effet (aucun pas d'annulation), calcule la rotation normalisée par son
//! propre `fmod`, et écrit lui-même la forme JSON attendue (clés omises quand vides). Il est
//! comparé au projet après **chaque** pas. Des **compteurs** prouvent que chaque contrôle fort
//! s'exécute. Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 117).

use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

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
    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize].clone()
    }
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

// ---------------------------------------------------------------------------
// Miroir
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct MPl {
    id: u64,
    piece: u64,
    size: Option<u64>,
    x: f64,
    y: f64,
    rot: f64,
    mir: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct MPlan {
    id: u64,
    name: String,
    material: Option<String>,
    width: f64,
    folded: bool,
    sel: f64,
    gap: f64,
    grain: Option<f64>,
    pls: Vec<MPl>,
}

#[derive(Clone, Debug)]
struct Model {
    plans: Vec<MPlan>,
    pieces: Vec<u64>,
    sizes: Vec<u64>,
    next_id: u64,
}

/// Paramètres demandés, tels que saisis (non rognés).
#[derive(Clone, Debug)]
struct SpecM {
    name: String,
    material: Option<String>,
    width: f64,
    folded: bool,
    sel: f64,
    gap: f64,
    grain: Option<f64>,
}

impl SpecM {
    fn command_spec(&self) -> CuttingPlanSpec {
        CuttingPlanSpec {
            name: self.name.clone(),
            material: self.material.clone(),
            fabric_width_mm: self.width,
            fold: if self.folded {
                FabricFold::Folded
            } else {
                FabricFold::Flat
            },
            selvedge_mm: self.sel,
            min_gap_mm: self.gap,
            grain_direction: self.grain,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cat {
    NameEmpty,
    NameTaken,
    MaterialEmpty,
    Width,
    Selvedge,
    Gap,
    Grain,
    UnknownPlan,
    UnknownPiece,
    UnknownSize,
    Coord,
    Rotation,
}

impl Cat {
    /// Mot que le message de refus doit contenir.
    fn word(self) -> &'static str {
        match self {
            Cat::NameEmpty | Cat::MaterialEmpty => "vide",
            Cat::NameTaken => "existe déjà",
            Cat::Width => "laize",
            Cat::Selvedge => "lisière",
            Cat::Gap => "écart minimal",
            Cat::Grain => "droit-fil",
            Cat::UnknownPlan => "plan de coupe introuvable",
            Cat::UnknownPiece => "",
            Cat::UnknownSize => "taille",
            Cat::Coord => "placement",
            Cat::Rotation => "rotation",
        }
    }
}

/// `fmod` ramené dans [0 ; 360[ (écrit ici, sans `rem_euclid`).
fn norm_rot(v: f64) -> f64 {
    let mut r = v % 360.0;
    if r < 0.0 {
        r += 360.0;
    }
    if r >= 360.0 {
        r = 0.0;
    }
    r + 0.0
}

fn lower(s: &str) -> String {
    s.trim().to_lowercase()
}

impl Model {
    fn spec_cat(&self, except: Option<u64>, s: &SpecM) -> Option<Cat> {
        if s.name.trim().is_empty() {
            return Some(Cat::NameEmpty);
        }
        if s.material.as_ref().is_some_and(|m| m.trim().is_empty()) {
            return Some(Cat::MaterialEmpty);
        }
        if self
            .plans
            .iter()
            .any(|p| Some(p.id) != except && lower(&p.name) == lower(&s.name))
        {
            return Some(Cat::NameTaken);
        }
        if !(s.width.is_finite() && s.width > 0.0 && s.width <= 10_000.0) {
            return Some(Cat::Width);
        }
        if !(s.sel.is_finite() && s.sel >= 0.0) || 2.0 * s.sel >= s.width {
            return Some(Cat::Selvedge);
        }
        if !(s.gap.is_finite() && (0.0..=1000.0).contains(&s.gap)) {
            return Some(Cat::Gap);
        }
        if let Some(g) = s.grain {
            if !(g.is_finite() && (0.0..360.0).contains(&g)) {
                return Some(Cat::Grain);
            }
        }
        None
    }

    fn stored(&self, id: u64, s: &SpecM, pls: Vec<MPl>) -> MPlan {
        MPlan {
            id,
            name: s.name.trim().to_string(),
            material: s.material.as_ref().map(|m| m.trim().to_string()),
            width: s.width,
            folded: s.folded,
            sel: s.sel,
            gap: s.gap,
            grain: s.grain,
            pls,
        }
    }

    /// Catégorie de refus d'un placement (`piece` n'est pas contrôlée pour une mise à jour).
    fn placement_cat(&self, plan: u64, piece: Option<u64>, pl: &MPl) -> Option<Cat> {
        if !self.plans.iter().any(|p| p.id == plan) {
            return Some(Cat::UnknownPlan);
        }
        if let Some(pc) = piece {
            if !self.pieces.contains(&pc) {
                return Some(Cat::UnknownPiece);
            }
        }
        if let Some(s) = pl.size {
            if !self.sizes.contains(&s) {
                return Some(Cat::UnknownSize);
            }
        }
        if !(pl.rot.is_finite() && pl.rot.abs() <= 1.0e6) {
            return Some(Cat::Rotation);
        }
        let ok = |v: f64| v.is_finite() && v.abs() <= 1.0e6;
        if !ok(pl.x) || !ok(pl.y) {
            return Some(Cat::Coord);
        }
        None
    }

    /// Forme JSON attendue du champ `cutting_plans` (`Null` quand il n'y a aucun plan : clé omise).
    fn expected_value(&self) -> Value {
        if self.plans.is_empty() {
            return Value::Null;
        }
        Value::Array(
            self.plans
                .iter()
                .map(|p| {
                    let mut m = serde_json::Map::new();
                    m.insert("id".into(), json!(p.id));
                    m.insert("name".into(), json!(p.name));
                    if let Some(x) = &p.material {
                        m.insert("material".into(), json!(x));
                    }
                    m.insert("fabric_width_mm".into(), json!(p.width));
                    m.insert(
                        "fold".into(),
                        json!(if p.folded { "folded" } else { "flat" }),
                    );
                    m.insert("selvedge_mm".into(), json!(p.sel));
                    m.insert("min_gap_mm".into(), json!(p.gap));
                    if let Some(g) = p.grain {
                        m.insert("grain_direction".into(), json!(g));
                    }
                    if !p.pls.is_empty() {
                        let pls: Vec<Value> = p
                            .pls
                            .iter()
                            .map(|q| {
                                let mut m = serde_json::Map::new();
                                m.insert("id".into(), json!(q.id));
                                m.insert("piece".into(), json!(q.piece));
                                if let Some(s) = q.size {
                                    m.insert("size".into(), json!(s));
                                }
                                m.insert("x_mm".into(), json!(q.x));
                                m.insert("y_mm".into(), json!(q.y));
                                m.insert("rotation_deg".into(), json!(q.rot));
                                if q.mir {
                                    m.insert("mirrored".into(), json!(true));
                                }
                                Value::Object(m)
                            })
                            .collect();
                        m.insert("placements".into(), Value::Array(pls));
                    }
                    Value::Object(m)
                })
                .collect(),
        )
    }
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    steps: usize,
    plans_added: usize,
    plans_updated: usize,
    plan_noops: usize,
    plans_removed: usize,
    placements_added: usize,
    placements_updated: usize,
    placement_noops: usize,
    placements_removed: usize,
    placements_normalized: usize,
    piece_deleted: usize,
    piece_refused: usize,
    size_removed: usize,
    size_refused: usize,
    refused: BTreeMap<u8, usize>,
    undo_checks: usize,
    redo_checks: usize,
    reload_checks: usize,
    state_checks: usize,
    json_checks: usize,
    id_checks: usize,
    plans_seen_nonempty: usize,
}

struct Seq<'a> {
    rng: &'a mut Rng,
    doc: Document,
    model: Model,
    undo: Vec<(Model, String)>,
    redo: Vec<(Model, String)>,
    /// Identifiants retirés dans cette suite linéaire (jamais réattribués tant qu'on n'annule pas).
    removed_ids: BTreeSet<u64>,
    /// Pièces et tailles du départ, taille de base (jamais retirée).
    all_pieces: Vec<u64>,
    all_sizes: Vec<u64>,
    base_size: u64,
    c: &'a mut Counters,
}

fn rect_piece(p: &mut Project, s: SheetId, x: f64, name: &str) -> PieceId {
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(x, 0.0)),
            width_mm: 100.0,
            height_mm: 40.0,
        })
        .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths {
            entities: r.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

const GHOST: u64 = 900_000;

impl<'a> Seq<'a> {
    fn new(rng: &'a mut Rng, c: &'a mut Counters) -> Seq<'a> {
        let mut p = Project::new("Campagne plans");
        let s = p.sheets[0].id;
        let n = 2 + rng.below(2) as usize;
        let pieces: Vec<u64> = (0..n)
            .map(|i| rect_piece(&mut p, s, 200.0 * i as f64, &format!("Pièce {i}")).0)
            .collect();
        p.apply(Command::AddSizeRange {
            name: "Gamme".into(),
            kind: SizeKind::Alphanumeric,
            labels: vec!["S".into(), "M".into(), "L".into(), "XL".into()],
            base: 1,
        })
        .unwrap();
        let sizes: Vec<u64> = p.sizes[0].sizes.iter().map(|z| z.id.0).collect();
        let model = Model {
            plans: Vec::new(),
            pieces: pieces.clone(),
            sizes: sizes.clone(),
            next_id: p.next_id,
        };
        Seq {
            rng,
            doc: Document::new(p),
            model,
            undo: Vec::new(),
            redo: Vec::new(),
            removed_ids: BTreeSet::new(),
            all_pieces: pieces,
            base_size: sizes[1],
            all_sizes: sizes,
            c,
        }
    }

    // --- tirages -----------------------------------------------------------

    /// Paramètres de plan : valides, ou avec **un seul** champ invalide.
    fn spec(&mut self) -> SpecM {
        let mut s = SpecM {
            name: self
                .rng
                .pick(&[
                    "Plan 1",
                    "Plan 2",
                    "Veste",
                    "Jupe longue",
                    "Doublure",
                    " Manteau ",
                ])
                .to_string(),
            material: self
                .rng
                .pick(&[None, Some("Coton"), Some(" Lin "), Some("Cuir de veau")])
                .map(str::to_string),
            width: self.rng.pick(&[1500.0, 1400.0, 10_000.0, 800.0, 1100.5]),
            folded: self.rng.chance(40),
            sel: self.rng.pick(&[0.0, 10.0, 20.0, 399.0]),
            gap: self.rng.pick(&[0.0, 5.0, 12.5, 1000.0]),
            grain: self.rng.pick(&[None, Some(0.0), Some(90.0), Some(359.9)]),
        };
        if self.rng.chance(60) {
            return s;
        }
        match self.rng.below(7) {
            0 => s.name = self.rng.pick(&["", "   "]).to_string(),
            1 => {
                // Nom déjà pris (casse et blancs ignorés) quand il y a un plan, sinon vide.
                s.name = match self.model.plans.first() {
                    Some(p) => format!(" {} ", p.name.to_uppercase()),
                    None => String::new(),
                };
            }
            2 => s.material = Some(self.rng.pick(&["", "  "]).to_string()),
            3 => {
                s.width = self
                    .rng
                    .pick(&[0.0, -5.0, f64::NAN, f64::INFINITY, 10_000.5])
            }
            4 => {
                // Lisière négative, non finie, ou telle que 2 × lisière ≥ laize.
                s.sel = self
                    .rng
                    .pick(&[-1.0, f64::NAN, f64::INFINITY, s.width / 2.0, s.width])
            }
            5 => s.gap = self.rng.pick(&[-1.0, 1000.5, f64::NAN, f64::INFINITY]),
            _ => {
                s.grain = Some(
                    self.rng
                        .pick(&[-0.5, 360.0, 400.0, f64::NAN, f64::NEG_INFINITY]),
                )
            }
        }
        s
    }

    fn alive_piece(&mut self) -> Option<u64> {
        if self.model.pieces.is_empty() {
            None
        } else {
            Some(self.rng.pick(&self.model.pieces.clone()))
        }
    }

    fn size_choice(&mut self) -> Option<u64> {
        if self.rng.chance(40) {
            None
        } else {
            Some(self.rng.pick(&self.model.sizes.clone()))
        }
    }

    /// Placement tiré : valide ou avec **un seul** champ invalide (id, pièce non contrôlées ici).
    fn placement(&mut self, piece: u64) -> (MPl, bool) {
        let ok_xy = [0.0, 12.5, -300.25, 1.0e6, -1.0e6, 850.125];
        let ok_rot = [0.0, 90.0, 450.0, -90.0, 359.5, 1.0e6, -1.0e-20, 360.0, 30.0];
        let mut pl = MPl {
            id: 0,
            piece,
            size: self.size_choice(),
            x: self.rng.pick(&ok_xy),
            y: self.rng.pick(&ok_xy),
            rot: self.rng.pick(&ok_rot),
            mir: self.rng.chance(50),
        };
        let mut invalid = false;
        if self.rng.chance(25) {
            invalid = true;
            match self.rng.below(5) {
                0 => pl.x = self.rng.pick(&[1.0e6 + 1.0, f64::NAN, -2.0e6]),
                1 => pl.y = self.rng.pick(&[-1.0e6 - 1.0, f64::INFINITY]),
                2 => pl.rot = self.rng.pick(&[f64::NAN, 1.0e6 + 1.0, f64::NEG_INFINITY]),
                3 => pl.size = Some(self.rng.pick(&[GHOST, GHOST + 7])),
                _ => pl.size = self.removed_size(),
            }
        }
        (pl, invalid)
    }

    /// Une taille réellement retirée si elle existe, sinon une taille fantôme.
    fn removed_size(&self) -> Option<u64> {
        let dead = self
            .all_sizes
            .iter()
            .copied()
            .find(|z| !self.model.sizes.contains(z));
        Some(dead.unwrap_or(GHOST + 1))
    }

    // --- vérifications -----------------------------------------------------

    fn before(&self) -> (Model, String) {
        (self.model.clone(), bytes(self.doc.project()))
    }

    /// Compare le projet au miroir : plans (forme JSON), pièces, tailles, compteur, validation.
    fn check_state(&mut self) {
        let text = bytes(self.doc.project());
        let doc: Value = serde_json::from_str(&text).unwrap();
        let got = doc.get("cutting_plans").cloned().unwrap_or(Value::Null);
        assert_eq!(
            got,
            self.model.expected_value(),
            "plans différents du miroir"
        );
        let p = self.doc.project();
        let pieces: Vec<u64> = p.pieces.iter().map(|x| x.id.0).collect();
        assert_eq!(pieces, self.model.pieces, "pièces vivantes");
        let sizes: Vec<u64> = p.sizes[0].sizes.iter().map(|z| z.id.0).collect();
        assert_eq!(sizes, self.model.sizes, "tailles vivantes");
        assert_eq!(p.next_id, self.model.next_id, "compteur d'identifiants");
        p.validate().unwrap();
        if !self.model.plans.is_empty() {
            self.c.plans_seen_nonempty += 1;
        } else {
            assert!(!text.contains("cutting_plans"), "clé écrite sans plan");
        }
        self.c.state_checks += 1;
        self.c.json_checks += 1;
    }

    /// Exécute `cmd` ; `expect` = verdict du miroir. Renvoie l'issue de la commande.
    fn run(
        &mut self,
        cmd: Command,
        refusal: Option<Cat>,
        next: Option<Model>,
    ) -> Result<Outcome, CoreError> {
        let (m0, b0) = self.before();
        let res = self.doc.execute(cmd);
        match (refusal, res) {
            (Some(cat), Err(e)) => {
                assert_eq!(bytes(self.doc.project()), b0, "refus avec trace ({cat:?})");
                let m = e.to_string();
                assert!(m.contains(cat.word()), "{cat:?} : {m}");
                *self.c.refused.entry(cat as u8).or_default() += 1;
                Err(e)
            }
            (Some(cat), Ok(_)) => panic!("la commande devait être refusée ({cat:?})"),
            (None, Err(e)) => panic!("la commande devait aboutir : {e}"),
            (None, Ok(out)) => {
                let next = next.expect("miroir suivant");
                let effective = next_differs(&m0, &next);
                if effective {
                    self.undo.push((m0, b0));
                    self.redo.clear();
                    self.model = next;
                } else {
                    assert_eq!(bytes(self.doc.project()), b0, "pas sans effet");
                }
                Ok(out)
            }
        }
    }

    fn step(&mut self) {
        self.c.steps += 1;
        let roll = self.rng.below(100);
        match roll {
            0..=14 => self.add_plan(),
            15..=24 => self.update_plan(),
            25..=29 => self.remove_plan(),
            30..=54 => self.add_placement(),
            55..=69 => self.update_placement(),
            70..=76 => self.remove_placement(),
            77..=82 => self.delete_piece(),
            83..=88 => self.remove_size(),
            89..=93 => self.undo_step(),
            94..=97 => self.redo_step(),
            _ => self.reload(),
        }
        self.check_state();
    }

    fn add_plan(&mut self) {
        let s = self.spec();
        let cat = self.model.spec_cat(None, &s);
        let id = self.model.next_id;
        let next = cat.is_none().then(|| {
            let mut m = self.model.clone();
            m.plans.push(m.stored(id, &s, Vec::new()));
            m.next_id += 1;
            m
        });
        let next_before = self.doc.project().next_id;
        if let Ok(out) = self.run(
            Command::AddCuttingPlan {
                spec: s.command_spec(),
            },
            cat,
            next,
        ) {
            self.c.plans_added += 1;
            assert_eq!(out.cutting_plans, vec![CuttingPlanId(id)]);
            self.check_new_id(id, next_before);
        }
    }

    fn check_new_id(&mut self, id: u64, next_before: u64) {
        assert_eq!(id, next_before, "identifiant attendu = compteur");
        assert!(!self.removed_ids.contains(&id), "identifiant réutilisé");
        self.c.id_checks += 1;
    }

    fn pick_plan(&mut self) -> Option<MPlan> {
        if self.model.plans.is_empty() {
            None
        } else {
            Some(self.rng.pick(&self.model.plans.clone()))
        }
    }

    fn update_plan(&mut self) {
        let Some(plan) = self.pick_plan() else {
            return self.add_plan();
        };
        let ghost = self.rng.chance(6);
        let target = if ghost { GHOST } else { plan.id };
        let s = if !ghost && self.rng.chance(30) {
            // Mêmes valeurs (éventuellement avec des blancs autour des textes).
            SpecM {
                name: format!(" {} ", plan.name),
                material: plan.material.clone().map(|m| format!("{m} ")),
                width: plan.width,
                folded: plan.folded,
                sel: plan.sel,
                gap: plan.gap,
                grain: plan.grain,
            }
        } else {
            self.spec()
        };
        let cat = if ghost {
            Some(Cat::UnknownPlan)
        } else {
            self.model.spec_cat(Some(plan.id), &s)
        };
        let next = cat.is_none().then(|| {
            let mut m = self.model.clone();
            let slot = m.plans.iter_mut().find(|p| p.id == plan.id).unwrap();
            let pls = std::mem::take(&mut slot.pls);
            *slot = self.model.stored(plan.id, &s, pls);
            m
        });
        let before_steps = self.undo.len();
        if self
            .run(
                Command::UpdateCuttingPlan {
                    plan: CuttingPlanId(target),
                    spec: s.command_spec(),
                },
                cat,
                next,
            )
            .is_ok()
        {
            if self.undo.len() > before_steps {
                self.c.plans_updated += 1;
            } else {
                self.c.plan_noops += 1;
            }
        }
    }

    fn remove_plan(&mut self) {
        let Some(plan) = self.pick_plan() else {
            return self.add_plan();
        };
        let ghost = self.rng.chance(6);
        let target = if ghost { GHOST } else { plan.id };
        let cat = ghost.then_some(Cat::UnknownPlan);
        let next = (!ghost).then(|| {
            let mut m = self.model.clone();
            m.plans.retain(|p| p.id != plan.id);
            m
        });
        if self
            .run(
                Command::RemoveCuttingPlan {
                    plan: CuttingPlanId(target),
                },
                cat,
                next,
            )
            .is_ok()
        {
            self.c.plans_removed += 1;
            self.removed_ids.insert(plan.id);
            self.removed_ids.extend(plan.pls.iter().map(|q| q.id));
        }
    }

    fn add_placement(&mut self) {
        let Some(plan) = self.pick_plan() else {
            return self.add_plan();
        };
        let dead_piece = self.rng.chance(7);
        let ghost_plan = self.rng.chance(5);
        let dead = self
            .all_pieces
            .iter()
            .copied()
            .find(|z| !self.model.pieces.contains(z))
            .unwrap_or(GHOST + 3);
        let piece = match (dead_piece, self.alive_piece()) {
            (false, Some(p)) => p,
            _ => dead,
        };
        let (mut pl, _) = self.placement(piece);
        let target = if ghost_plan { GHOST } else { plan.id };
        let cat = if ghost_plan {
            Some(Cat::UnknownPlan)
        } else {
            self.model.placement_cat(plan.id, Some(piece), &pl)
        };
        let id = self.model.next_id;
        pl.id = id;
        let next = cat.is_none().then(|| {
            let mut m = self.model.clone();
            let mut stored = pl.clone();
            stored.rot = norm_rot(pl.rot);
            if stored.rot != pl.rot {
                self.c.placements_normalized += 1;
            }
            stored.x += 0.0;
            stored.y += 0.0;
            m.plans
                .iter_mut()
                .find(|p| p.id == plan.id)
                .unwrap()
                .pls
                .push(stored);
            m.next_id += 1;
            m
        });
        let next_before = self.doc.project().next_id;
        if let Ok(out) = self.run(
            Command::AddPlacement {
                plan: CuttingPlanId(target),
                piece: PieceId(piece),
                size: pl.size.map(SizeId),
                x_mm: pl.x,
                y_mm: pl.y,
                rotation_deg: pl.rot,
                mirrored: pl.mir,
            },
            cat,
            next,
        ) {
            self.c.placements_added += 1;
            assert_eq!(out.placements, vec![PlacementId(id)]);
            self.check_new_id(id, next_before);
        }
    }

    fn pick_placement(&mut self) -> Option<(MPlan, MPl)> {
        let all: Vec<(MPlan, MPl)> = self
            .model
            .plans
            .iter()
            .flat_map(|p| p.pls.iter().map(move |q| (p.clone(), q.clone())))
            .collect();
        if all.is_empty() {
            None
        } else {
            Some(self.rng.pick(&all))
        }
    }

    fn update_placement(&mut self) {
        let Some((plan, old)) = self.pick_placement() else {
            return self.add_placement();
        };
        let (new, _) = if self.rng.chance(35) {
            // Mêmes valeurs ; la rotation est rendue équivalente (+ 360) une fois sur deux.
            let mut same = old.clone();
            if self.rng.chance(50) {
                same.rot += 360.0;
            }
            (same, false)
        } else {
            self.placement(old.piece)
        };
        let cat = self.model.placement_cat(plan.id, None, &new);
        let next = cat.is_none().then(|| {
            let mut m = self.model.clone();
            let slot = m
                .plans
                .iter_mut()
                .find(|p| p.id == plan.id)
                .unwrap()
                .pls
                .iter_mut()
                .find(|q| q.id == old.id)
                .unwrap();
            *slot = MPl {
                id: old.id,
                piece: old.piece,
                size: new.size,
                x: new.x + 0.0,
                y: new.y + 0.0,
                rot: norm_rot(new.rot),
                mir: new.mir,
            };
            m
        });
        let before_steps = self.undo.len();
        if self
            .run(
                Command::UpdatePlacement {
                    plan: CuttingPlanId(plan.id),
                    placement: PlacementId(old.id),
                    size: new.size.map(SizeId),
                    x_mm: new.x,
                    y_mm: new.y,
                    rotation_deg: new.rot,
                    mirrored: new.mir,
                },
                cat,
                next,
            )
            .is_ok()
        {
            if self.undo.len() > before_steps {
                self.c.placements_updated += 1;
            } else {
                self.c.placement_noops += 1;
            }
        }
    }

    fn remove_placement(&mut self) {
        let Some((plan, pl)) = self.pick_placement() else {
            return self.add_placement();
        };
        let ghost = self.rng.chance(8);
        let target = if ghost { GHOST } else { pl.id };
        // Refus d'un placement inconnu : message « placement introuvable ».
        let cmd = Command::RemovePlacement {
            plan: CuttingPlanId(plan.id),
            placement: PlacementId(target),
        };
        if ghost {
            let (_, b0) = self.before();
            let e = self.doc.execute(cmd).expect_err("placement fantôme");
            assert!(e.to_string().contains("placement introuvable"), "{e}");
            assert_eq!(bytes(self.doc.project()), b0);
            *self.c.refused.entry(100).or_default() += 1;
            return;
        }
        let mut m = self.model.clone();
        m.plans
            .iter_mut()
            .find(|p| p.id == plan.id)
            .unwrap()
            .pls
            .retain(|q| q.id != pl.id);
        if self.run(cmd, None, Some(m)).is_ok() {
            self.c.placements_removed += 1;
            self.removed_ids.insert(pl.id);
        }
    }

    /// Premier plan (dans l'ordre) qui pose `piece` ou la taille `size`.
    fn first_plan_using(&self, piece: Option<u64>, size: Option<u64>) -> Option<String> {
        self.model
            .plans
            .iter()
            .find(|p| {
                p.pls
                    .iter()
                    .any(|q| Some(q.piece) == piece || (size.is_some() && q.size == size))
            })
            .map(|p| p.name.clone())
    }

    fn delete_piece(&mut self) {
        let Some(piece) = self.alive_piece() else {
            return self.add_placement();
        };
        let used = self.first_plan_using(Some(piece), None);
        let (m0, b0) = self.before();
        let res = self.doc.execute(Command::DeletePiece {
            piece: PieceId(piece),
        });
        match (used, res) {
            (Some(name), Err(e)) => {
                assert!(e.to_string().contains(&name), "plan non nommé : {e}");
                assert_eq!(bytes(self.doc.project()), b0, "refus avec trace");
                self.c.piece_refused += 1;
            }
            (None, Ok(_)) => {
                let mut m = m0.clone();
                m.pieces.retain(|p| *p != piece);
                self.undo.push((m0, b0));
                self.redo.clear();
                self.model = m;
                self.c.piece_deleted += 1;
            }
            (Some(_), Ok(_)) => panic!("pièce posée supprimée"),
            (None, Err(e)) => panic!("pièce libre non supprimée : {e}"),
        }
    }

    fn remove_size(&mut self) {
        // La taille de base n'est jamais retirée (le retrait de la base est une autre règle).
        let removable: Vec<u64> = self
            .model
            .sizes
            .iter()
            .copied()
            .filter(|s| *s != self.base_size)
            .collect();
        if removable.is_empty() {
            return self.add_placement();
        }
        let size = self.rng.pick(&removable);
        let used = self.first_plan_using(None, Some(size));
        let (m0, b0) = self.before();
        let res = self.doc.execute(Command::RemoveSize { size: SizeId(size) });
        match (used, res) {
            (Some(name), Err(e)) => {
                assert!(e.to_string().contains(&name), "plan non nommé : {e}");
                assert_eq!(bytes(self.doc.project()), b0, "refus avec trace");
                self.c.size_refused += 1;
            }
            (None, Ok(_)) => {
                let mut m = m0.clone();
                m.sizes.retain(|s| *s != size);
                self.undo.push((m0, b0));
                self.redo.clear();
                self.model = m;
                self.removed_ids.insert(size);
                self.c.size_removed += 1;
            }
            (Some(_), Ok(_)) => panic!("taille posée retirée"),
            (None, Err(e)) => panic!("taille libre non retirée : {e}"),
        }
    }

    fn undo_step(&mut self) {
        let current = (self.model.clone(), bytes(self.doc.project()));
        let ok = self.doc.undo();
        match self.undo.pop() {
            Some((m, b)) => {
                assert!(ok, "annulation refusée alors qu'une étape existe");
                // Octet exact avec l'état d'avant la commande annulée.
                assert_eq!(bytes(self.doc.project()), b, "annulation inexacte");
                self.redo.push(current);
                self.model = m;
                self.removed_ids.clear();
                self.c.undo_checks += 1;
                // Souvent : on rétablit tout de suite (aller-retour exact).
                if self.rng.chance(60) {
                    self.redo_step();
                }
            }
            None => assert!(!ok, "annulation sans étape"),
        }
    }

    fn redo_step(&mut self) {
        let current = (self.model.clone(), bytes(self.doc.project()));
        let ok = self.doc.redo();
        match self.redo.pop() {
            Some((m, b)) => {
                assert!(ok, "rétablissement refusé alors qu'une étape existe");
                assert_eq!(bytes(self.doc.project()), b, "rétablissement inexact");
                self.undo.push(current);
                self.model = m;
                self.removed_ids.clear();
                self.c.redo_checks += 1;
            }
            None => assert!(!ok, "rétablissement sans étape"),
        }
    }

    fn reload(&mut self) {
        let text = bytes(self.doc.project());
        let again = persist::from_json(&text).unwrap();
        assert_eq!(&again, self.doc.project(), "relecture différente");
        assert_eq!(bytes(&again), text, "relecture non idempotente");
        self.c.reload_checks += 1;
    }
}

/// Deux états du miroir diffèrent-ils (plans, pièces, tailles, compteur) ?
fn next_differs(a: &Model, b: &Model) -> bool {
    a.plans != b.plans || a.pieces != b.pieces || a.sizes != b.sizes || a.next_id != b.next_id
}

#[test]
fn campagne_adverse_plans_de_coupe() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut c = Counters::default();
    for _ in 0..170 {
        c.sequences += 1;
        let len = 14 + rng.below(17) as usize;
        let mut seq = Seq::new(&mut rng, &mut c);
        seq.check_state();
        for _ in 0..len {
            seq.step();
        }
    }
    eprintln!("{c:#?}");
    assert_eq!(c.sequences, 170);
    assert!(c.steps >= 170 * 14);
    // Chaque catégorie de refus a été exercée, avec la bonne raison.
    for cat in [
        Cat::NameEmpty,
        Cat::NameTaken,
        Cat::MaterialEmpty,
        Cat::Width,
        Cat::Selvedge,
        Cat::Gap,
        Cat::Grain,
        Cat::UnknownPlan,
        Cat::UnknownPiece,
        Cat::UnknownSize,
        Cat::Coord,
        Cat::Rotation,
    ] {
        assert!(
            c.refused.get(&(cat as u8)).copied().unwrap_or(0) >= 5,
            "refus {cat:?} trop rare : {:?}",
            c.refused
        );
    }
    assert!(c.refused.get(&100).copied().unwrap_or(0) >= 5);
    // Acceptations et cas sans effet.
    assert!(c.plans_added >= 300, "{}", c.plans_added);
    assert!(c.plans_updated >= 50);
    assert!(c.plan_noops >= 20);
    assert!(c.plans_removed >= 50);
    assert!(c.placements_added >= 300);
    assert!(c.placements_updated >= 100);
    assert!(c.placement_noops >= 30);
    assert!(c.placements_removed >= 50);
    assert!(c.placements_normalized >= 100);
    // Suppressions de pièce et de taille : refusées (posées) et acceptées (libres).
    assert!(c.piece_refused >= 30, "{}", c.piece_refused);
    assert!(c.piece_deleted >= 20);
    assert!(c.size_refused >= 30, "{}", c.size_refused);
    assert!(c.size_removed >= 20);
    // Contrôles forts exécutés.
    assert!(c.undo_checks >= 150, "{}", c.undo_checks);
    assert!(c.redo_checks >= 100, "{}", c.redo_checks);
    assert!(c.reload_checks >= 50);
    assert!(c.id_checks >= 600);
    assert!(c.state_checks >= 170 * 14);
    assert!(c.plans_seen_nonempty >= 1500);
}
