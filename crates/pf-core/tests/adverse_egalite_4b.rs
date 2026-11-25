//! US-03.4b — campagne adverse « égalité de longueurs étendue » : 170 séquences à graine fixe de 60
//! pas sur 3 feuilles (2 segments et 2 courbes droites chacune) : poser une égalité « courbe =
//! somme de 1 à 3 tracés de toutes feuilles + décalage (nombre ou formule) », l'ancienne égalité à
//! un tracé, une valeur, délier, déplacer un point de segment, changer une variable, supprimer un
//! tracé ou une feuille, réordonner les feuilles, annuler, rétablir, relire le fichier.
//!
//! **Oracle indépendant** : un petit monde tenu ici (segments = distance entre deux extrémités,
//! courbes droites = longueur de leur corde sur l'axe X, formules évaluées à la main) qui n'appelle
//! ni `pf_core::length_constraint` ni `pf_geometry::solve`. Pour chaque commande l'oracle prédit :
//! refus (cycle, cible <= 0, aucune longueur imposée, tracé cité deux fois, auto-référence, tracé
//! inconnu) ou acceptation avec la longueur de chaque courbe (récursion avec détection de cycle,
//! indépendante du tri topologique du noyau). Exigences : refus = octets identiques ; après
//! acceptation, extrémité de chaque courbe à `longueur prévue` (1e-6), cible écrite = prévue, valeur
//! du décalage évaluée = prévue, contraintes du projet = contraintes prévues (suppressions : la
//! contrainte dont un tracé disparaît est retirée, la longueur reste) ; étape si et seulement si
//! l'état change ; annuler / rétablir exacts à l'octet ; relecture identique. Compteurs de
//! couverture assertés. Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 115).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::collections::HashSet;

const SEQUENCES: u64 = 170;
const STEPS: usize = 60;
const MAX_LEN: f64 = 20_000.0;
const BORDER: f64 = 0.01;

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
    fn val(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 100.0) as u64;
        lo + self.below(n + 1) as f64 / 100.0
    }
    fn pick(&mut self, n: usize) -> usize {
        self.below(n as u64) as usize
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

type Xy = (f64, f64);

#[derive(Clone, Debug)]
struct OSeg {
    id: EntityId,
    pa: PointId,
    pb: PointId,
    a: Xy,
    b: Xy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Off {
    Num(f64),
    /// Formule n° k sur les variables `va`, `vb`.
    Form(u8),
}

const FORMS: [&str; 5] = ["va*2", "-vb", "va+vb-3", "va", "(va-vb)/2"];

#[derive(Clone, Debug, PartialEq)]
enum Tgt {
    Num(f64),
    Eq {
        terms: Vec<(SheetId, EntityId)>,
        off: Off,
    },
}

#[derive(Clone, Debug)]
struct OCurve {
    id: EntityId,
    end: PointId,
    y: f64,
    len: f64,
    con: Option<Tgt>,
}

#[derive(Clone, Debug)]
struct OSheet {
    id: SheetId,
    segs: Vec<OSeg>,
    curves: Vec<OCurve>,
}

#[derive(Clone, Debug)]
struct World {
    sheets: Vec<OSheet>,
    vars: [f64; 2],
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Verdict {
    Cycle,
    NonPositive,
    /// Trop proche d'une frontière ou trop long : le générateur n'envoie pas la commande.
    Skip,
}

fn dist(a: Xy, b: Xy) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

impl World {
    fn off_value(&self, off: Off) -> f64 {
        let (a, b) = (self.vars[0], self.vars[1]);
        match off {
            Off::Num(o) => o,
            Off::Form(0) => a * 2.0,
            Off::Form(1) => -b,
            Off::Form(2) => a + b - 3.0,
            Off::Form(3) => a,
            Off::Form(_) => (a - b) / 2.0,
        }
    }

    fn curve(&self, id: EntityId) -> Option<(&OSheet, &OCurve)> {
        self.sheets
            .iter()
            .find_map(|s| s.curves.iter().find(|c| c.id == id).map(|c| (s, c)))
    }

    fn entities(&self) -> Vec<(SheetId, EntityId, bool)> {
        let mut out = Vec::new();
        for s in &self.sheets {
            out.extend(s.segs.iter().map(|g| (s.id, g.id, false)));
            out.extend(s.curves.iter().map(|c| (s.id, c.id, true)));
        }
        out
    }

    /// Longueur de chaque courbe après résolution, ou le verdict de refus.
    fn solve(&self) -> Result<Vec<(EntityId, f64)>, Verdict> {
        let mut memo: Vec<(EntityId, f64)> = Vec::new();
        let ids: Vec<EntityId> = self
            .sheets
            .iter()
            .flat_map(|s| s.curves.iter().map(|c| c.id))
            .collect();
        for id in ids {
            self.eval(id, &mut memo, &mut Vec::new())?;
        }
        Ok(memo)
    }

    fn eval(
        &self,
        id: EntityId,
        memo: &mut Vec<(EntityId, f64)>,
        stack: &mut Vec<EntityId>,
    ) -> Result<f64, Verdict> {
        if let Some((_, v)) = memo.iter().find(|(e, _)| *e == id) {
            return Ok(*v);
        }
        if stack.contains(&id) {
            return Err(Verdict::Cycle);
        }
        let (_, c) = self.curve(id).expect("courbe");
        let v = match &c.con {
            None => c.len,
            Some(Tgt::Num(v)) => *v,
            Some(Tgt::Eq { terms, off }) => {
                stack.push(id);
                let mut sum = 0.0;
                for &(sheet, e) in terms {
                    let s = self.sheets.iter().find(|s| s.id == sheet).expect("feuille");
                    if let Some(g) = s.segs.iter().find(|g| g.id == e) {
                        sum += dist(g.a, g.b);
                    } else {
                        sum += self.eval(e, memo, stack)?;
                    }
                }
                stack.pop();
                sum + self.off_value(*off)
            }
        };
        if v < BORDER && v > -BORDER {
            return Err(Verdict::Skip);
        }
        if v <= 0.0 {
            return Err(Verdict::NonPositive);
        }
        if v > MAX_LEN {
            return Err(Verdict::Skip);
        }
        memo.push((id, v));
        Ok(v)
    }

    /// Applique les longueurs prévues.
    fn commit(&mut self, lens: &[(EntityId, f64)]) {
        for s in &mut self.sheets {
            for c in &mut s.curves {
                if let Some((_, v)) = lens.iter().find(|(e, _)| *e == c.id) {
                    c.len = *v;
                }
            }
        }
    }

    /// Retire les contraintes dont un tracé (ou la courbe) a disparu.
    fn retire_dead(&mut self) {
        let alive: HashSet<(SheetId, EntityId)> = self
            .entities()
            .into_iter()
            .map(|(s, e, _)| (s, e))
            .collect();
        for s in &mut self.sheets {
            for c in &mut s.curves {
                let dead = matches!(&c.con, Some(Tgt::Eq { terms, .. }) if terms.iter().any(|t| !alive.contains(t)));
                if dead {
                    c.con = None;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Moteur de la campagne
// ---------------------------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counters {
    accepted: u64,
    refused: u64,
    cross_sheet_terms: u64,
    sums_of_several: u64,
    formula_offsets: u64,
    cross_sheet_chains: u64,
    cycles_refused: u64,
    cycles_confirmed: u64,
    non_positive_refused: u64,
    no_constraint_refused: u64,
    duplicate_refused: u64,
    self_refused: u64,
    unknown_refused: u64,
    old_style: u64,
    unlinks: u64,
    numeric_over_equality: u64,
    variable_changes: u64,
    moves: u64,
    deletions_retiring: u64,
    sheet_deletions: u64,
    sheet_moves: u64,
    undos: u64,
    redos: u64,
    rereads: u64,
    noops: u64,
}

struct Run {
    doc: Document,
    w: World,
    states: Vec<String>,
    snapshots: Vec<World>,
    cursor: usize,
    seq: u64,
    step: usize,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

impl Run {
    fn ctx(&self) -> String {
        format!("séquence {} pas {}", self.seq, self.step)
    }

    fn project(&self) -> &Project {
        self.doc.project()
    }

    /// Refus attendu : projet identique à l'octet. Renvoie `true` si le message dit « circulaire ».
    /// Un cycle et une cible <= 0 peuvent coexister : le noyau cite alors l'un ou l'autre.
    fn expect_refused(&mut self, cmd: Command, why: &str) -> bool {
        let before = bytes(self.project());
        let err = self.doc.execute(cmd.clone());
        assert!(
            err.is_err(),
            "{} : l'oracle prédit un refus ({why}), le noyau accepte {cmd:?}",
            self.ctx()
        );
        assert_eq!(
            bytes(self.project()),
            before,
            "{} : trace d'un refus",
            self.ctx()
        );
        let message = err.unwrap_err().to_string();
        if why == "cycle" {
            assert!(
                message.contains("circulaire") || message.contains("pas de solution"),
                "{} : message de cycle : {message}",
                self.ctx()
            );
        } else {
            assert!(
                !message.contains("circulaire") || why.contains("<= 0"),
                "{} : refus ({why}) présenté comme un cycle : {message}",
                self.ctx()
            );
        }
        message.contains("circulaire")
    }

    /// Acceptation attendue ; `next` est le monde prévu.
    fn expect_accepted(&mut self, cmd: Command, next: World, c: &mut Counters) {
        let before = bytes(self.project());
        if let Err(e) = self.doc.execute(cmd.clone()) {
            panic!(
                "{} : l'oracle prédit l'acceptation, le noyau refuse ({e}) : {cmd:?}",
                self.ctx()
            );
        }
        self.w = next;
        self.verify();
        let after = bytes(self.project());
        if after == before {
            c.noops += 1;
        } else {
            self.push_state(after);
        }
        c.accepted += 1;
    }

    fn push_state(&mut self, after: String) {
        self.states.truncate(self.cursor + 1);
        self.snapshots.truncate(self.cursor + 1);
        self.states.push(after);
        self.snapshots.push(self.w.clone());
        self.cursor += 1;
    }

    /// Le projet correspond au monde de l'oracle.
    fn verify(&self) {
        let p = self.project();
        assert_eq!(p.sheets.len(), self.w.sheets.len(), "{}", self.ctx());
        for os in &self.w.sheets {
            let sheet = p
                .sheet(os.id)
                .unwrap_or_else(|e| panic!("{} : {e}", self.ctx()));
            let mut cons = 0;
            for oc in &os.curves {
                let end = sheet.position(oc.end).unwrap();
                assert!(
                    (end.x - oc.len).abs() < 1e-6 && (end.y - oc.y).abs() < 1e-9,
                    "{} : courbe {:?} : extrémité {end:?}, longueur prévue {}",
                    self.ctx(),
                    oc.id,
                    oc.len
                );
                let k = sheet.length_constraint(oc.id);
                match (&oc.con, k) {
                    (None, None) => {}
                    (Some(Tgt::Num(v)), Some(k)) => {
                        cons += 1;
                        assert!(k.reference.is_none() && k.link.is_none(), "{}", self.ctx());
                        assert!((k.length_mm - v).abs() < 1e-6, "{}", self.ctx());
                    }
                    (Some(Tgt::Eq { terms, off }), Some(k)) => {
                        cons += 1;
                        assert!((k.length_mm - oc.len).abs() < 1e-6, "{}", self.ctx());
                        let got: Vec<(SheetId, EntityId)> = k
                            .terms()
                            .iter()
                            .map(|t| (t.sheet.unwrap_or(os.id), t.entity))
                            .collect();
                        assert_eq!(&got, terms, "{}", self.ctx());
                        let want = self.w.off_value(*off);
                        assert!(
                            (k.reference.unwrap().offset_mm - want).abs() < 1e-9,
                            "{} : décalage écrit {}, prévu {want}",
                            self.ctx(),
                            k.reference.unwrap().offset_mm
                        );
                        match off {
                            Off::Num(_) => assert!(k.offset_expr().is_none()),
                            Off::Form(n) => {
                                assert_eq!(k.offset_expr(), Some(FORMS[*n as usize]))
                            }
                        }
                        // Forme canonique : jamais l'extension quand l'égalité est simple.
                        let simple =
                            terms.len() == 1 && terms[0].0 == os.id && matches!(off, Off::Num(_));
                        assert_eq!(k.link.is_none(), simple, "{}", self.ctx());
                        assert!(k.terms().iter().all(|t| t.sheet != Some(os.id)));
                    }
                    (want, got) => panic!(
                        "{} : contrainte de {:?} prévue {want:?}, trouvée {got:?}",
                        self.ctx(),
                        oc.id
                    ),
                }
            }
            assert_eq!(sheet.length_constraints.len(), cons, "{}", self.ctx());
        }
        // Relecture : le fichier relu est identique.
        let text = bytes(p);
        assert_eq!(
            bytes(&persist::from_json(&text).unwrap_or_else(|e| panic!("{} : {e}", self.ctx()))),
            text,
            "{}",
            self.ctx()
        );
    }
}

fn setup(rng: &mut Rng) -> (Run, Vec<Vec<(PointId, PointId)>>) {
    let mut p = Project::new("Campagne 4b");
    let mut ids = vec![p.sheets[0].id];
    for i in 1..3 {
        ids.push(
            p.apply(Command::CreateSheet {
                name: format!("Feuille {i}"),
            })
            .unwrap()
            .sheet
            .unwrap(),
        );
    }
    let vars = [rng.val(2.0, 30.0), rng.val(2.0, 30.0)];
    for (name, v) in [("va", vars[0]), ("vb", vars[1])] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit: VariableUnit::Mm,
            expr: format!("{v}"),
        })
        .unwrap();
    }
    let mut sheets = Vec::new();
    let mut seg_points = Vec::new();
    for (si, &sid) in ids.iter().enumerate() {
        let mut segs = Vec::new();
        let mut pts = Vec::new();
        for _ in 0..2 {
            let a = (rng.val(0.0, 100.0), rng.val(200.0, 260.0));
            let b = (a.0 + rng.val(20.0, 150.0), a.1 + rng.val(-40.0, 40.0));
            let o = p
                .apply(Command::AddSegment {
                    sheet: sid,
                    from: PointRef::New(PointMm::new(a.0, a.1)),
                    to: PointRef::New(PointMm::new(b.0, b.1)),
                })
                .unwrap();
            segs.push(OSeg {
                id: o.entities[0],
                pa: o.points[0],
                pb: o.points[1],
                a,
                b,
            });
            pts.push((o.points[0], o.points[1]));
        }
        let mut curves = Vec::new();
        for k in 0..2 {
            let y = 20.0 * (si * 2 + k) as f64;
            let x = rng.val(80.0, 200.0);
            let o = p
                .apply(Command::AddCurve {
                    sheet: sid,
                    start: PointRef::New(PointMm::new(0.0, y)),
                    c1: PointMm::new(0.0, y),
                    c2: PointMm::new(x, y),
                    end: PointRef::New(PointMm::new(x, y)),
                })
                .unwrap();
            curves.push(OCurve {
                id: o.entities[0],
                end: o.points[1],
                y,
                len: x,
                con: None,
            });
        }
        sheets.push(OSheet {
            id: sid,
            segs,
            curves,
        });
        seg_points.push(pts);
    }
    let first = bytes(&p);
    let run = Run {
        doc: Document::new(p),
        w: World {
            sheets: sheets.clone(),
            vars,
        },
        states: vec![first],
        snapshots: vec![World { sheets, vars }],
        cursor: 0,
        seq: 0,
        step: 0,
    };
    run.verify();
    (run, seg_points)
}

fn off_command(off: Off) -> (f64, Option<String>) {
    match off {
        Off::Num(o) => (o, None),
        Off::Form(n) => (0.0, Some(FORMS[n as usize].to_string())),
    }
}

fn sequence(seq: u64, c: &mut Counters) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (seq + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
    for _ in 0..4 {
        rng.step();
    }
    let (mut run, _) = setup(&mut rng);
    run.seq = seq;
    for step in 0..STEPS {
        run.step = step;
        let w = run.w.clone();
        let curves: Vec<(SheetId, EntityId, PointId)> = w
            .sheets
            .iter()
            .flat_map(|s| s.curves.iter().map(|k| (s.id, k.id, k.end)))
            .collect();
        let action = rng.below(100);
        if curves.is_empty() && action <= 63 {
            continue;
        }
        match action {
            // Égalité étendue.
            0..=39 => {
                if curves.is_empty() {
                    continue;
                }
                let (sid, cid, _) = curves[rng.pick(curves.len())];
                let all = w.entities();
                let n = 1 + rng.pick(3);
                let mut terms: Vec<(SheetId, EntityId)> = Vec::new();
                let mut flavour = "";
                for _ in 0..n {
                    let (s, e, _) = all[rng.pick(all.len())];
                    terms.push((s, e));
                }
                if rng.chance(6) {
                    terms[0] = (sid, cid);
                    flavour = "self";
                } else if rng.chance(5) {
                    terms[0] = (sid, EntityId(900_000 + rng.below(50)));
                    flavour = "unknown";
                } else if rng.chance(6) && terms.len() >= 2 {
                    terms[1] = terms[0];
                    flavour = "dup";
                } else {
                    // Sans doublon ni auto-référence : retire les répétitions.
                    let mut seen = HashSet::new();
                    terms.retain(|t| *t != (sid, cid) && seen.insert(*t));
                    if terms.is_empty() {
                        continue;
                    }
                }
                let off = if rng.chance(40) {
                    Off::Form(rng.below(FORMS.len() as u64) as u8)
                } else {
                    Off::Num(rng.val(-80.0, 80.0))
                };
                let (offset_mm, offset_expr) = off_command(off);
                // Une même feuille s'écrit sans feuille ou avec la sienne : même forme canonique.
                let wire_terms: Vec<LengthTerm> = terms
                    .iter()
                    .map(|&(s, e)| LengthTerm {
                        entity: e,
                        sheet: if s != sid || rng.chance(50) {
                            Some(s).filter(|s| *s != sid)
                        } else {
                            Some(sid)
                        },
                    })
                    .collect();
                let cmd = Command::SetCurveLengthEquality {
                    sheet: sid,
                    curve: cid,
                    equality: Some(LengthEquality {
                        terms: wire_terms,
                        offset_mm,
                        offset_expr,
                    }),
                };
                let has_con = w.curve(cid).unwrap().1.con.is_some();
                match flavour {
                    "self" => {
                        run.expect_refused(cmd, "auto-référence");
                        c.self_refused += 1;
                        c.refused += 1;
                        continue;
                    }
                    "unknown" => {
                        run.expect_refused(cmd, "tracé inconnu");
                        c.unknown_refused += 1;
                        c.refused += 1;
                        continue;
                    }
                    "dup" => {
                        run.expect_refused(cmd, "doublon");
                        c.duplicate_refused += 1;
                        c.refused += 1;
                        continue;
                    }
                    _ => {}
                }
                if !has_con {
                    run.expect_refused(cmd, "aucune longueur imposée");
                    c.no_constraint_refused += 1;
                    c.refused += 1;
                    continue;
                }
                let mut next = w.clone();
                for s in &mut next.sheets {
                    for k in &mut s.curves {
                        if k.id == cid {
                            k.con = Some(Tgt::Eq {
                                terms: terms.clone(),
                                off,
                            });
                        }
                    }
                }
                match next.solve() {
                    Err(Verdict::Skip) => continue,
                    Err(Verdict::Cycle) => {
                        if run.expect_refused(cmd, "cycle") {
                            c.cycles_confirmed += 1;
                        }
                        c.cycles_refused += 1;
                        c.refused += 1;
                    }
                    Err(Verdict::NonPositive) => {
                        run.expect_refused(cmd, "cible <= 0");
                        c.non_positive_refused += 1;
                        c.refused += 1;
                    }
                    Ok(lens) => {
                        next.commit(&lens);
                        if terms.iter().any(|t| t.0 != sid) {
                            c.cross_sheet_terms += 1;
                            if terms
                                .iter()
                                .any(|t| next.curve(t.1).is_some_and(|(_, k)| k.con.is_some()))
                            {
                                c.cross_sheet_chains += 1;
                            }
                        }
                        if terms.len() > 1 {
                            c.sums_of_several += 1;
                        }
                        if matches!(off, Off::Form(_)) {
                            c.formula_offsets += 1;
                        }
                        run.expect_accepted(cmd, next, c);
                    }
                }
            }
            // Ancienne égalité à un tracé de la même feuille (SetCurveLengthConstraint).
            40..=47 => {
                let (sid, cid, end) = curves[rng.pick(curves.len())];
                let s = w.sheets.iter().find(|s| s.id == sid).unwrap();
                let mut cands: Vec<EntityId> = s.segs.iter().map(|g| g.id).collect();
                cands.extend(s.curves.iter().map(|k| k.id).filter(|&k| k != cid));
                if cands.is_empty() {
                    continue;
                }
                let e = cands[rng.pick(cands.len())];
                let o = rng.val(-60.0, 60.0);
                let cmd = Command::SetCurveLengthConstraint {
                    sheet: sid,
                    curve: cid,
                    spec: Some(LengthSpec {
                        length_mm: 1.0,
                        adjust: LengthAdjust::Slide {
                            point: end,
                            angle_deg: 0.0,
                        },
                        reference: Some(LengthRef {
                            entity: e,
                            offset_mm: o,
                        }),
                    }),
                };
                let mut next = w.clone();
                for sh in &mut next.sheets {
                    for k in &mut sh.curves {
                        if k.id == cid {
                            k.con = Some(Tgt::Eq {
                                terms: vec![(sid, e)],
                                off: Off::Num(o),
                            });
                        }
                    }
                }
                match next.solve() {
                    Err(Verdict::Skip) => continue,
                    Err(Verdict::Cycle) => {
                        if run.expect_refused(cmd, "cycle") {
                            c.cycles_confirmed += 1;
                        }
                        c.cycles_refused += 1;
                        c.refused += 1;
                    }
                    Err(Verdict::NonPositive) => {
                        run.expect_refused(cmd, "cible <= 0");
                        c.non_positive_refused += 1;
                        c.refused += 1;
                    }
                    Ok(lens) => {
                        next.commit(&lens);
                        c.old_style += 1;
                        run.expect_accepted(cmd, next, c);
                    }
                }
            }
            // Valeur imposée (remplace une égalité).
            48..=57 => {
                let (sid, cid, end) = curves[rng.pick(curves.len())];
                let v = rng.val(20.0, 300.0);
                let cmd = Command::SetCurveLengthConstraint {
                    sheet: sid,
                    curve: cid,
                    spec: Some(LengthSpec {
                        length_mm: v,
                        adjust: LengthAdjust::Slide {
                            point: end,
                            angle_deg: 0.0,
                        },
                        reference: None,
                    }),
                };
                let mut next = w.clone();
                let was_eq = matches!(w.curve(cid).unwrap().1.con, Some(Tgt::Eq { .. }));
                for sh in &mut next.sheets {
                    for k in &mut sh.curves {
                        if k.id == cid {
                            k.con = Some(Tgt::Num(v));
                        }
                    }
                }
                match next.solve() {
                    Err(Verdict::Cycle) => {
                        // Imposer une valeur peut défaire un cycle, jamais en créer.
                        panic!("{} : cycle prédit pour une valeur", run.ctx());
                    }
                    Err(_) => continue,
                    Ok(lens) => {
                        next.commit(&lens);
                        if was_eq {
                            c.numeric_over_equality += 1;
                        }
                        run.expect_accepted(cmd, next, c);
                    }
                }
            }
            // Délier l'égalité : la valeur courante est gardée.
            58..=63 => {
                let (sid, cid, _) = curves[rng.pick(curves.len())];
                let cmd = Command::SetCurveLengthEquality {
                    sheet: sid,
                    curve: cid,
                    equality: None,
                };
                let cur = w.curve(cid).unwrap().1;
                let mut next = w.clone();
                for sh in &mut next.sheets {
                    for k in &mut sh.curves {
                        if k.id == cid {
                            if let Some(Tgt::Eq { .. }) = &k.con {
                                k.con = Some(Tgt::Num(cur.len));
                            }
                        }
                    }
                }
                let has_con = cur.con.is_some();
                if !has_con {
                    // Aucune contrainte : le noyau refuse (« aucune longueur imposée »).
                    run.expect_refused(cmd, "aucune longueur imposée");
                    c.no_constraint_refused += 1;
                    c.refused += 1;
                    continue;
                }
                if matches!(cur.con, Some(Tgt::Eq { .. })) {
                    c.unlinks += 1;
                }
                match next.solve() {
                    Ok(lens) => {
                        next.commit(&lens);
                        run.expect_accepted(cmd, next, c);
                    }
                    Err(_) => continue,
                }
            }
            // Déplacer une extrémité de segment.
            64..=73 => {
                let all: Vec<(SheetId, &OSeg)> = w
                    .sheets
                    .iter()
                    .flat_map(|s| s.segs.iter().map(move |g| (s.id, g)))
                    .collect();
                if all.is_empty() {
                    continue;
                }
                let (sid, g) = all[rng.pick(all.len())];
                let first = rng.chance(50);
                let to = (rng.val(0.0, 250.0), rng.val(200.0, 260.0));
                let other = if first { g.b } else { g.a };
                if dist(to, other) < 5.0 {
                    continue;
                }
                let cmd = Command::MovePoint {
                    sheet: sid,
                    point: if first { g.pa } else { g.pb },
                    to: PointMm::new(to.0, to.1),
                };
                let mut next = w.clone();
                for sh in &mut next.sheets {
                    for s in &mut sh.segs {
                        if s.id == g.id {
                            if first {
                                s.a = to;
                            } else {
                                s.b = to;
                            }
                        }
                    }
                }
                match next.solve() {
                    Err(Verdict::Skip) => continue,
                    Err(_) => {
                        run.expect_refused(cmd, "cible <= 0");
                        c.non_positive_refused += 1;
                        c.refused += 1;
                    }
                    Ok(lens) => {
                        next.commit(&lens);
                        c.moves += 1;
                        run.expect_accepted(cmd, next, c);
                    }
                }
            }
            // Changer une variable.
            74..=80 => {
                let which = rng.pick(2);
                let v = rng.val(2.0, 40.0);
                let name = ["va", "vb"][which];
                let cmd = Command::SetVariableExpr {
                    name: name.into(),
                    expr: format!("{v}"),
                };
                let mut next = w.clone();
                next.vars[which] = v;
                match next.solve() {
                    Err(Verdict::Skip) => continue,
                    Err(_) => {
                        run.expect_refused(cmd, "cible <= 0");
                        c.non_positive_refused += 1;
                        c.refused += 1;
                    }
                    Ok(lens) => {
                        next.commit(&lens);
                        c.variable_changes += 1;
                        run.expect_accepted(cmd, next, c);
                    }
                }
            }
            // Supprimer un segment ou une courbe.
            81..=86 => {
                let all = w.entities();
                if all.is_empty() {
                    continue;
                }
                let (sid, e, is_curve) = all[rng.pick(all.len())];
                let cmd = Command::Delete {
                    sheet: sid,
                    points: vec![],
                    entities: vec![e],
                };
                let mut next = w.clone();
                let referenced = next.sheets.iter().any(|s| {
                        s.curves.iter().any(|k| {
                            matches!(&k.con, Some(Tgt::Eq { terms, .. }) if terms.contains(&(sid, e)))
                        })
                    });
                for sh in &mut next.sheets {
                    if sh.id == sid {
                        sh.segs.retain(|g| g.id != e);
                        sh.curves.retain(|k| k.id != e);
                    }
                }
                next.retire_dead();
                match next.solve() {
                    Ok(lens) => {
                        next.commit(&lens);
                        if referenced || is_curve {
                            c.deletions_retiring += 1;
                        }
                        run.expect_accepted(cmd, next, c);
                    }
                    Err(_) => continue,
                }
            }
            // Supprimer une feuille.
            87..=88 => {
                if w.sheets.len() < 2 {
                    continue;
                }
                let sid = w.sheets[rng.pick(w.sheets.len())].id;
                let cmd = Command::DeleteSheet { sheet: sid };
                let mut next = w.clone();
                next.sheets.retain(|s| s.id != sid);
                next.retire_dead();
                match next.solve() {
                    Ok(lens) => {
                        next.commit(&lens);
                        c.sheet_deletions += 1;
                        run.expect_accepted(cmd, next, c);
                    }
                    Err(_) => continue,
                }
            }
            // Réordonner les feuilles (l'ordre de résolution ne dépend pas de l'index).
            89..=91 => {
                let sid = w.sheets[rng.pick(w.sheets.len())].id;
                let cmd = Command::MoveSheet {
                    sheet: sid,
                    new_index: rng.pick(w.sheets.len()),
                };
                let before = bytes(run.project());
                run.doc.execute(cmd).unwrap();
                run.w = w.clone();
                run.verify();
                let after = bytes(run.project());
                if after != before {
                    run.push_state(after);
                    c.sheet_moves += 1;
                }
            }
            // Annuler / rétablir.
            92..=95 => {
                if run.cursor > 0 {
                    assert!(run.doc.undo(), "{}", run.ctx());
                    run.cursor -= 1;
                    assert_eq!(
                        bytes(run.project()),
                        run.states[run.cursor],
                        "{}",
                        run.ctx()
                    );
                    sync_world(&mut run);
                    c.undos += 1;
                }
            }
            96..=98 => {
                if run.cursor + 1 < run.states.len() {
                    assert!(run.doc.redo(), "{}", run.ctx());
                    run.cursor += 1;
                    assert_eq!(
                        bytes(run.project()),
                        run.states[run.cursor],
                        "{}",
                        run.ctx()
                    );
                    sync_world(&mut run);
                    c.redos += 1;
                }
            }
            _ => {
                let text = bytes(run.project());
                let back = persist::from_json(&text).unwrap();
                assert_eq!(bytes(&back), text, "{}", run.ctx());
                c.rereads += 1;
            }
        }
    }
    // Toutes les étapes se défont jusqu'au projet de départ, et pas une de plus.
    while run.cursor > 0 {
        assert!(run.doc.undo(), "{}", run.ctx());
        run.cursor -= 1;
        assert_eq!(
            bytes(run.project()),
            run.states[run.cursor],
            "{}",
            run.ctx()
        );
    }
    assert!(!run.doc.can_undo(), "séquence {seq} : étape de trop");
}

/// Après annuler / rétablir, le monde de l'oracle est relu du projet (états déjà vérifiés à
/// l'octet) : on retrouve l'état d'oracle correspondant en le recalculant depuis le projet.
fn sync_world(run: &mut Run) {
    // L'oracle est reconstruit à partir de l'état enregistré à ce curseur.
    let snapshot = run.snapshots[run.cursor].clone();
    run.w = snapshot;
    run.verify();
}

#[test]
fn campagne_adverse_egalite_longueurs_etendue() {
    let mut c = Counters::default();
    for seq in 0..SEQUENCES {
        sequence(seq, &mut c);
    }
    eprintln!("{c:?}");
    assert!(c.accepted > 3000, "{c:?}");
    assert!(c.refused > 300, "{c:?}");
    assert!(c.cross_sheet_terms > 500, "{c:?}");
    assert!(c.sums_of_several > 500, "{c:?}");
    assert!(c.formula_offsets > 400, "{c:?}");
    assert!(c.cross_sheet_chains > 100, "{c:?}");
    assert!(c.cycles_refused > 30 && c.cycles_confirmed > 20, "{c:?}");
    assert!(c.non_positive_refused > 30, "{c:?}");
    assert!(c.no_constraint_refused > 30, "{c:?}");
    assert!(c.duplicate_refused > 30, "{c:?}");
    assert!(c.self_refused > 30, "{c:?}");
    assert!(c.unknown_refused > 30, "{c:?}");
    assert!(c.old_style > 100, "{c:?}");
    assert!(c.unlinks > 50, "{c:?}");
    assert!(c.numeric_over_equality > 50, "{c:?}");
    assert!(c.variable_changes > 300, "{c:?}");
    assert!(c.moves > 400, "{c:?}");
    assert!(c.deletions_retiring > 100, "{c:?}");
    assert!(c.sheet_deletions > 30, "{c:?}");
    assert!(c.sheet_moves > 30, "{c:?}");
    assert!(c.undos > 100 && c.redos > 20, "{c:?}");
    assert!(c.rereads > 30, "{c:?}");
}
