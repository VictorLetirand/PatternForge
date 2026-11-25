//! US-03.6 — campagne adverse « historique de construction » : 160 séquences à graine fixe de 60
//! pas (variables, points construits, formules, segments maintenus, courbes, longueurs imposées et
//! égalités, rectangles et pièces, crans, cotations, éléments internes, marquages, boutons,
//! pinces, dérivées, surpiqûres, coutures, suppressions, calques et verrous, annuler, rétablir).
//!
//! **Oracle indépendant** : le graphe de dépendances est reconstruit ici **depuis le JSON** du
//! projet (`serde_json::to_value`), sans appeler `pf_core::dependency` : nœuds par type d'objet,
//! arêtes « parent → enfant » par type de définition de point, de lien et de pièce, noms de
//! nature écrits dans ce fichier, variables citées lues par un découpage en identifiants du
//! texte (pas l'analyseur du cœur), parcours en largeur écrit ici.
//!
//! Invariants après **chaque** pas (accepté, refusé, annuler, rétablir) :
//! - le graphe du cœur (`dependency_edges`) est celui de l'oracle ; l'arbre de quelques objets
//!   tirés au sort (profondeurs, natures directes, feuilles) est celui du parcours de l'oracle,
//!   en profondeur illimitée comme bornée ;
//! - les fonctions sont pures (octets du projet inchangés, deux appels égaux) ;
//! - sur une suppression envisagée (1 à 3 objets au hasard) : **impact annoncé == effet réel** de
//!   la commande sur une copie : refus identique, objets supprimés (hors extrémités libres
//!   nettoyées) et extrémités nettoyées lus par différence des deux JSON, liens retirés = arêtes
//!   de l'oracle perdues dont l'enfant survit ; « trivial » = ni supprimé ni lien retiré (une
//!   remarque peut exister sans conséquence : la formule d'un point supprimé) ; l'exécution réelle puis l'annulation
//!   (une étape) rendent exactement les octets d'avant ; le projet relu donne le même impact.
//!
//! Mutation vérifiée à la main : voir `docs/JOURNAL.md` (tranche 93).

use pf_core::persist;
use pf_core::DependencyNature as N;
use pf_core::DependencyObject as O;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

const SEQUENCES: u64 = 160;
const STEPS: usize = 60;

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
    fn pick<T: Clone>(&mut self, xs: &[T]) -> Option<T> {
        if xs.is_empty() {
            None
        } else {
            Some(xs[self.below(xs.len() as u64) as usize].clone())
        }
    }
    fn val(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 100.0) as u64;
        lo + self.below(n + 1) as f64 / 100.0
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle (lecture du JSON)
// ---------------------------------------------------------------------------------------------

/// Arête de l'oracle : (parent, enfant, nature). Les nœuds sont des textes « genre:identifiant ».
type OEdge = (String, String, &'static str);

#[derive(Default)]
struct Graph {
    nodes: BTreeMap<String, Option<u64>>,
    edges: BTreeSet<OEdge>,
}

fn arr(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

fn num(v: &Value) -> u64 {
    v.as_u64()
        .unwrap_or_else(|| panic!("identifiant attendu : {v}"))
}

fn idents(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.insert(cs[start..i].iter().collect());
        } else if c.is_ascii_digit() {
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == ',' || cs[i] == '.') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

fn graph(p: &Project) -> Graph {
    let v = serde_json::to_value(p).unwrap();
    let mut g = Graph::default();
    let mut raw: Vec<OEdge> = Vec::new();
    let pt = |v: &Value| format!("point:{}", num(v));
    let en = |v: &Value| format!("entity:{}", num(v));
    let pc = |v: &Value| format!("piece:{}", num(v));
    let var_names: HashSet<String> = arr(&v["variables"])
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect();
    for var in arr(&v["variables"]) {
        let name = var["name"].as_str().unwrap();
        g.nodes.insert(format!("variable:{name}"), None);
        for cited in idents(var["expr"].as_str().unwrap()) {
            if var_names.contains(&cited) {
                raw.push((
                    format!("variable:{cited}"),
                    format!("variable:{name}"),
                    "variable_ref",
                ));
            }
        }
    }
    // Tailles (US-09.2a) : nœuds sans feuille ; un écart relie sa taille à son point.
    for range in arr(&v["sizes"]) {
        for size in arr(&range["sizes"]) {
            g.nodes.insert(format!("size:{}", num(&size["id"])), None);
        }
    }
    // Feuille d'une pièce : liée = sa feuille ; détachée = celle de son premier tracé.
    let mut entity_sheet: HashMap<u64, u64> = HashMap::new();
    for s in arr(&v["sheets"]) {
        let sid = num(&s["id"]);
        for e in arr(&s["entities"]) {
            entity_sheet.insert(num(&e["id"]), sid);
        }
    }
    let piece_sheet = |piece: &Value| -> Option<u64> {
        if piece["link"]["type"] == "linked" {
            Some(num(&piece["link"]["sheet"]))
        } else {
            piece["outline"]
                .as_array()
                .and_then(|o| o.first())
                .and_then(|e| entity_sheet.get(&num(&e["entity"])).copied())
        }
    };
    for s in arr(&v["sheets"]) {
        let sid = Some(num(&s["id"]));
        for p in arr(&s["points"]) {
            let id = num(&p["id"]);
            g.nodes.insert(format!("point:{id}"), sid);
            let d = &p["definition"];
            let child = format!("point:{id}");
            let mut ents: Vec<&Value> = Vec::new();
            let mut pts: Vec<&Value> = Vec::new();
            match d["type"].as_str().unwrap() {
                "free" => {}
                "on_segment" | "on_segment_ratio" => ents.push(&d["segment"]),
                "intersection" | "circle_intersection" | "trace_intersection" => {
                    ents.push(&d["first"]);
                    ents.push(&d["second"]);
                }
                "on_curve" | "along_curve" => ents.push(&d["curve"]),
                "offset" => pts.push(&d["from"]),
                "midpoint" => {
                    pts.push(&d["first"]);
                    pts.push(&d["second"]);
                }
                "projection" => {
                    pts.push(&d["point"]);
                    ents.push(&d["trace"]);
                }
                "bisector" => {
                    pts.push(&d["vertex"]);
                    pts.push(&d["first"]);
                    pts.push(&d["second"]);
                }
                "polar" => {
                    pts.push(&d["from"]);
                    if !d["reference"].is_null() {
                        ents.push(&d["reference"]);
                    }
                }
                "compass" => {
                    pts.push(&d["center_a"]);
                    pts.push(&d["center_b"]);
                }
                "division" => {
                    ents.push(&d["trace"]);
                    if !d["start"].is_null() {
                        pts.push(&d["start"]);
                    }
                }
                "mirror" => {
                    pts.push(&d["source"]);
                    match d["axis"]["type"].as_str().unwrap() {
                        "guide" => raw.push((
                            format!("guide:{}", num(&d["axis"]["guide"])),
                            child.clone(),
                            "construction",
                        )),
                        _ => ents.push(&d["axis"]["segment"]),
                    }
                }
                other => panic!("définition inconnue de l'oracle : {other}"),
            }
            for e in ents {
                raw.push((en(e), child.clone(), "construction"));
            }
            for q in pts {
                raw.push((pt(q), child.clone(), "construction"));
            }
        }
        for gd in arr(&s["guides"]) {
            g.nodes.insert(format!("guide:{}", num(&gd["id"])), sid);
        }
        for e in arr(&s["entities"]) {
            let id = num(&e["id"]);
            g.nodes.insert(format!("entity:{id}"), sid);
            let k = &e["kind"];
            let ends: Vec<&Value> = match k["type"].as_str().unwrap() {
                "segment" => vec![&k["from"], &k["to"]],
                "curve" => vec![&k["start"], &k["end"]],
                "circle" => vec![&k["center"]],
                "arc" => vec![&k["center"], &k["start"], &k["end"]],
                "spline" => arr(&k["nodes"]).map(|n| &n["point"]).collect(),
                other => panic!("tracé inconnu de l'oracle : {other}"),
            };
            for q in ends {
                raw.push((pt(q), format!("entity:{id}"), "path_point"));
            }
            if k["type"] == "segment" && k["construction"]["kept"] == Value::Bool(true) {
                let c = &k["construction"];
                let followed: Vec<&Value> = if c["type"] == "parallel" {
                    vec![&k["from"], &k["to"]]
                } else {
                    vec![&k["to"]]
                };
                let mut parents: Vec<String> = Vec::new();
                match c["type"].as_str().unwrap() {
                    "polar" => parents.push(pt(&k["from"])),
                    "perpendicular" => {
                        parents.push(pt(&k["from"]));
                        parents.push(en(&c["reference"]));
                    }
                    _ => parents.push(en(&c["reference"])),
                }
                for parent in parents {
                    for f in &followed {
                        raw.push((parent.clone(), pt(f), "kept"));
                    }
                }
            }
        }
        for g in arr(&s["gradings"]) {
            raw.push((
                format!("size:{}", num(&g["size"])),
                pt(&g["point"]),
                "size_grading",
            ));
        }
        for f in arr(&s["formulas"]) {
            for cited in idents(f["expr"].as_str().unwrap()) {
                if var_names.contains(&cited) {
                    raw.push((format!("variable:{cited}"), pt(&f["point"]), "formula_ref"));
                }
            }
        }
        for c in arr(&s["length_constraints"]) {
            if !c["reference"].is_null() {
                raw.push((
                    en(&c["reference"]["entity"]),
                    en(&c["curve"]),
                    "length_equality",
                ));
            }
            if c["adjust"]["type"] == "slide" {
                raw.push((en(&c["curve"]), pt(&c["adjust"]["point"]), "imposed_length"));
            }
        }
        for n in arr(&s["notches"]) {
            let id = format!("notch:{}", num(&n["id"]));
            g.nodes.insert(id.clone(), sid);
            raw.push((en(&n["entity"]), id.clone(), "notch_ref"));
            match n["anchor"]["type"].as_str() {
                Some("point") => raw.push((pt(&n["anchor"]["point"]), id.clone(), "notch_ref")),
                Some("carried") => {
                    raw.push((pt(&n["anchor"]["point"]), id.clone(), "notch_ref"));
                    raw.push((en(&n["anchor"]["entity"]), id.clone(), "notch_ref"));
                }
                _ => {}
            }
        }
        for d in arr(&s["dimensions"]) {
            let id = format!("dimension:{}", num(&d["id"]));
            g.nodes.insert(id.clone(), sid);
            raw.push((pt(&d["from"]), id.clone(), "dimension_ref"));
            raw.push((pt(&d["to"]), id.clone(), "dimension_ref"));
        }
        for d in arr(&s["full_dimensions"]) {
            let id = format!("fulldimension:{}", num(&d["id"]));
            g.nodes.insert(id.clone(), sid);
            let k = &d["kind"];
            match k["type"].as_str().unwrap() {
                "path_length" | "radius" | "diameter" => {
                    raw.push((en(&k["entity"]), id.clone(), "dimension_ref"))
                }
                "angle" => {
                    for key in ["vertex", "first", "second"] {
                        raw.push((pt(&k[key]), id.clone(), "dimension_ref"));
                    }
                }
                _ => {
                    raw.push((pt(&k["from"]), id.clone(), "dimension_ref"));
                    raw.push((pt(&k["to"]), id.clone(), "dimension_ref"));
                }
            }
        }
        for t in arr(&s["topstitches"]) {
            let id = format!("topstitch:{}", num(&t["id"]));
            g.nodes.insert(id.clone(), sid);
            if t["source"]["type"] == "piece" {
                raw.push((pc(&t["source"]["piece"]), id.clone(), "topstitch_source"));
            } else {
                for e in arr(&t["source"]["paths"]) {
                    raw.push((en(e), id.clone(), "topstitch_source"));
                }
                if !t["source"]["from"].is_null() {
                    raw.push((pt(&t["source"]["from"]), id.clone(), "topstitch_source"));
                }
            }
            for e in arr(&t["paths"]) {
                raw.push((id.clone(), en(e), "topstitch_line"));
            }
        }
    }
    for piece in arr(&v["pieces"]) {
        let id = format!("piece:{}", num(&piece["id"]));
        g.nodes.insert(id.clone(), piece_sheet(piece));
        for e in arr(&piece["outline"]) {
            raw.push((en(&e["entity"]), id.clone(), "piece_outline"));
        }
        if !piece["grainline"].is_null() {
            raw.push((en(&piece["grainline"]), id.clone(), "piece_grain"));
        }
        let grain = &piece["grain"];
        if !grain.is_null() {
            let mut dirs = vec![&grain["direction"]];
            for x in arr(&grain["extra"]) {
                raw.push((en(&x["entity"]), id.clone(), "piece_grain"));
                dirs.push(&x["direction"]);
            }
            for d in dirs {
                if matches!(d["mode"].as_str(), Some("parallel" | "perpendicular")) {
                    raw.push((en(&d["reference"]), id.clone(), "piece_grain"));
                }
            }
        }
        for e in arr(&piece["on_fold"]) {
            raw.push((en(e), id.clone(), "piece_fold"));
        }
        for i in arr(&piece["internal"]) {
            let t = &i["target"];
            let parent = match t["type"].as_str().unwrap() {
                "path" => en(&t["entity"]),
                "point" => pt(&t["point"]),
                _ => format!("notch:{}", num(&t["notch"])),
            };
            raw.push((parent, id.clone(), "piece_internal"));
        }
        for m in arr(&piece["markings"]) {
            let at = &m["at"];
            if at["type"] == "point" {
                raw.push((pt(&at["point"]), id.clone(), "piece_marking"));
            } else {
                raw.push((pt(&at["tip"]), id.clone(), "piece_marking"));
                raw.push((pt(&at["toward"]), id.clone(), "piece_marking"));
            }
        }
        for d in arr(&piece["darts"]) {
            raw.push((pt(&d["apex"]), id.clone(), "piece_dart"));
            raw.push((pt(&d["toward"]), id.clone(), "piece_dart"));
        }
        for b in arr(&piece["buttons"]) {
            raw.push((pt(&b["from"]), id.clone(), "piece_buttons"));
            raw.push((pt(&b["to"]), id.clone(), "piece_buttons"));
        }
        if !piece["derived"].is_null() {
            raw.push((pc(&piece["derived"]["mother"]), id.clone(), "piece_derived"));
            if piece["derived"]["operation"]["type"] == "unfold" {
                raw.push((
                    en(&piece["derived"]["operation"]["fold"]),
                    id.clone(),
                    "piece_derived",
                ));
            }
        }
    }
    for sp in arr(&v["seams"]) {
        let id = format!("seam:{}", num(&sp["id"]));
        let sheet = v["pieces"]
            .as_array()
            .and_then(|ps| ps.iter().find(|x| x["id"] == sp["a"]["piece"]))
            .and_then(piece_sheet);
        g.nodes.insert(id.clone(), sheet);
        for side in ["a", "b"] {
            raw.push((pc(&sp[side]["piece"]), id.clone(), "seam_pair"));
            for e in arr(&sp[side]["edges"]) {
                raw.push((en(e), id.clone(), "seam_pair"));
            }
        }
    }
    for (parent, child, nature) in raw {
        if parent != child && g.nodes.contains_key(&parent) && g.nodes.contains_key(&child) {
            g.edges.insert((parent, child, nature));
        }
    }
    g
}

/// Nom de nœud de l'oracle pour un objet du cœur.
fn node(o: &O) -> String {
    match o {
        O::Point(i) => format!("point:{}", i.0),
        O::Entity(i) => format!("entity:{}", i.0),
        O::Guide(i) => format!("guide:{}", i.0),
        O::Notch(i) => format!("notch:{}", i.0),
        O::Dimension(i) => format!("dimension:{}", i.0),
        O::FullDimension(i) => format!("fulldimension:{}", i.0),
        O::Topstitch(i) => format!("topstitch:{}", i.0),
        O::Piece(i) => format!("piece:{}", i.0),
        O::SeamPair(i) => format!("seam:{}", i.0),
        O::Variable(n) => format!("variable:{n}"),
        O::Size(i) => format!("size:{}", i.0),
    }
}

/// Nom de nature de l'oracle (écrit ici, pas `DependencyNature::key`).
fn nature(n: N) -> &'static str {
    match n {
        N::Construction => "construction",
        N::PathPoint => "path_point",
        N::KeptConstruction => "kept",
        N::FormulaReference => "formula_ref",
        N::VariableReference => "variable_ref",
        N::ImposedLength => "imposed_length",
        N::LengthEquality => "length_equality",
        N::NotchReference => "notch_ref",
        N::DimensionReference => "dimension_ref",
        N::TopstitchSource => "topstitch_source",
        N::TopstitchLine => "topstitch_line",
        N::PieceOutline => "piece_outline",
        N::PieceGrain => "piece_grain",
        N::PieceFold => "piece_fold",
        N::PieceInternal => "piece_internal",
        N::PieceMarking => "piece_marking",
        N::PieceDart => "piece_dart",
        N::PieceButtons => "piece_buttons",
        N::PieceDerived => "piece_derived",
        N::SeamPair => "seam_pair",
        N::SizeGrading => "size_grading",
    }
}

fn core_edge(e: &DependencyEdge) -> OEdge {
    (node(&e.parent), node(&e.child), nature(e.nature))
}

/// Ordre documenté des natures (celui de l'affichage) : sert à choisir la nature d'un lien direct
/// quand deux liens relient les mêmes objets.
const NATURE_ORDER: [&str; 21] = [
    "construction",
    "path_point",
    "kept",
    "formula_ref",
    "variable_ref",
    "imposed_length",
    "length_equality",
    "notch_ref",
    "dimension_ref",
    "topstitch_source",
    "topstitch_line",
    "piece_outline",
    "piece_grain",
    "piece_fold",
    "piece_internal",
    "piece_marking",
    "piece_dart",
    "piece_buttons",
    "piece_derived",
    "seam_pair",
    "size_grading",
];

fn rank(n: &&'static str) -> usize {
    NATURE_ORDER.iter().position(|x| x == n).unwrap()
}

/// Parcours en largeur de l'oracle : profondeur de chaque objet atteint, dans un sens.
/// `up` : parents ; sinon enfants. Renvoie aussi la nature minimale de l'arête directe.
fn oracle_walk(
    g: &Graph,
    from: &str,
    up: bool,
    max_depth: Option<usize>,
) -> BTreeMap<String, (usize, Option<&'static str>)> {
    let mut depth: BTreeMap<String, (usize, Option<&'static str>)> = BTreeMap::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    queue.push_back((from.to_string(), 0));
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(from.to_string());
    while let Some((cur, d)) = queue.pop_front() {
        if max_depth.is_some_and(|m| d >= m) {
            continue;
        }
        for (parent, child, nat) in &g.edges {
            let (here, other) = if up { (child, parent) } else { (parent, child) };
            if here != &cur {
                continue;
            }
            if d == 0 {
                let entry = depth.entry(other.clone()).or_insert((1, Some(nat)));
                entry.1 = entry.1.into_iter().chain(Some(*nat)).min_by_key(rank);
            }
            if seen.insert(other.clone()) {
                if d > 0 {
                    depth.insert(other.clone(), (d + 1, None));
                }
                queue.push_back((other.clone(), d + 1));
            }
        }
    }
    depth
}

// ---------------------------------------------------------------------------------------------
// Génération de commandes
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Cov {
    steps: u64,
    accepted: u64,
    refused: u64,
    undone: u64,
    redone: u64,
    trees: u64,
    impacts: u64,
    impact_refused: u64,
    impact_trivial: u64,
    impact_removed: u64,
    impact_cleaned: u64,
    impact_unlinked: u64,
    impact_notes: u64,
    impact_multi: u64,
    natures: BTreeSet<&'static str>,
    max_edges: usize,
    max_depth: usize,
    pieces: u64,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn piece_inside(sheet: &Sheet, piece: &Piece) -> Option<(PointMm, f64)> {
    // Centre et demi-hauteur de la boîte des sommets du contour.
    let mut pts: Vec<PointMm> = Vec::new();
    for e in &piece.outline {
        for q in sheet.entity_all_points(e.entity).ok()? {
            pts.push(sheet.position(q).ok()?);
        }
    }
    let (mut lo, mut hi) = (
        PointMm::new(f64::MAX, f64::MAX),
        PointMm::new(f64::MIN, f64::MIN),
    );
    for q in &pts {
        lo = PointMm::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = PointMm::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    (!pts.is_empty()).then(|| {
        (
            PointMm::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0),
            (hi.y - lo.y) / 2.0,
        )
    })
}

fn random_command(rng: &mut Rng, p: &Project, next_var: &mut u64) -> Command {
    let sheet = &p.sheets[rng.below(p.sheets.len() as u64) as usize];
    let sid = sheet.id;
    let points: Vec<PointId> = sheet.points.iter().map(|x| x.id).collect();
    let entities: Vec<EntityId> = sheet.entities.iter().map(|e| e.id).collect();
    let segments: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Segment { .. }))
        .map(|e| e.id)
        .collect();
    let constructed: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EntityKind::Segment {
                    construction: Some(_),
                    ..
                }
            )
        })
        .map(|e| e.id)
        .collect();
    let curves: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Curve { .. }))
        .map(|e| e.id)
        .collect();
    let pieces_here: Vec<&Piece> = p
        .pieces
        .iter()
        .filter(|x| x.link == PieceLink::Linked { sheet: sid })
        .collect();
    let vars: Vec<String> = p.variables.iter().map(|v| v.name.clone()).collect();
    let free = |rng: &mut Rng| PointMm::new(rng.val(-200.0, 200.0), rng.val(-200.0, 200.0));
    let side = |rng: &mut Rng| {
        if rng.chance(50) {
            Side::Left
        } else {
            Side::Right
        }
    };
    let fallback = |rng: &mut Rng| Command::AddPoint {
        sheet: sid,
        at: PointMm::new(rng.val(-200.0, 200.0), rng.val(-200.0, 200.0)),
    };
    let roll = rng.below(100);
    match roll {
        0..=4 => {
            *next_var += 1;
            let name = format!("v{next_var}");
            let expr = match rng.pick(&vars) {
                Some(other) if rng.chance(50) => format!("{other}*{}", 1 + rng.below(3)),
                _ => format!("{},{}", 5 + rng.below(80), rng.below(10)),
            };
            Command::AddVariable {
                name,
                unit: VariableUnit::Mm,
                expr,
            }
        }
        5..=6 => match rng.pick(&vars) {
            Some(name) => Command::RemoveVariable { name },
            None => fallback(rng),
        },
        7..=8 => match rng.pick(&vars) {
            Some(name) => Command::SetVariableExpr {
                name,
                expr: format!("{}", 5 + rng.below(90)),
            },
            None => fallback(rng),
        },
        9..=13 => fallback(rng),
        14..=23 => match (rng.pick(&points), rng.pick(&points), rng.pick(&segments)) {
            (Some(a), Some(b), seg) => {
                let def = match rng.below(5) {
                    0 => PointDefinition::Polar {
                        from: a,
                        distance_mm: rng.val(10.0, 120.0),
                        angle_deg: rng.val(0.0, 350.0),
                        reference: None,
                    },
                    1 => PointDefinition::Offset {
                        from: a,
                        dx_mm: rng.val(-80.0, 80.0),
                        dy_mm: rng.val(-80.0, 80.0),
                    },
                    2 if a != b => PointDefinition::Midpoint {
                        first: a,
                        second: b,
                    },
                    3 if seg.is_some() => PointDefinition::Projection {
                        point: a,
                        trace: seg.unwrap(),
                    },
                    _ => match seg {
                        Some(segment) => PointDefinition::OnSegmentRatio {
                            segment,
                            ratio: rng.val(0.1, 0.9),
                        },
                        None => PointDefinition::Offset {
                            from: a,
                            dx_mm: 10.0,
                            dy_mm: 10.0,
                        },
                    },
                };
                Command::AddPointAt {
                    sheet: sid,
                    point: PointRef::Derived(def),
                }
            }
            _ => fallback(rng),
        },
        24..=29 => {
            let targets: Vec<(PointId, bool)> = sheet
                .points
                .iter()
                .filter_map(|x| match x.definition {
                    PointDefinition::Polar { .. } => Some((x.id, true)),
                    PointDefinition::Offset { .. } => Some((x.id, false)),
                    _ => None,
                })
                .collect();
            match rng.pick(&targets) {
                Some((point, polar)) => {
                    let param = match (polar, rng.chance(50)) {
                        (true, true) => ParamKind::Distance,
                        (true, false) => ParamKind::Angle,
                        (false, true) => ParamKind::Dx,
                        (false, false) => ParamKind::Dy,
                    };
                    let expr = if rng.chance(20) {
                        None
                    } else {
                        Some(match rng.pick(&vars) {
                            Some(v) => format!("{v}/2+{}", 1 + rng.below(30)),
                            None => format!("{}", 5 + rng.below(40)),
                        })
                    };
                    Command::SetParameterExpr {
                        target: ParamTarget {
                            sheet: sid,
                            point,
                            param,
                        },
                        expr,
                    }
                }
                None => fallback(rng),
            }
        }
        30..=34 => Command::AddSegmentPolar {
            sheet: sid,
            from: match rng.pick(&points) {
                Some(f) if rng.chance(70) => PointRef::Existing(f),
                _ => PointRef::New(free(rng)),
            },
            length_mm: rng.val(20.0, 150.0),
            angle_deg: rng.val(0.0, 350.0),
        },
        35..=37 => match (rng.pick(&segments), rng.pick(&points)) {
            (Some(reference), Some(from)) => Command::AddPerpendicular {
                sheet: sid,
                reference,
                from: PointRef::Existing(from),
                length_mm: rng.val(20.0, 100.0),
                side: side(rng),
            },
            _ => fallback(rng),
        },
        38..=39 => match rng.pick(&segments) {
            Some(reference) => Command::AddParallel {
                sheet: sid,
                reference,
                distance_mm: rng.val(10.0, 60.0),
                side: side(rng),
            },
            None => fallback(rng),
        },
        40..=44 => match rng.pick(&constructed) {
            Some(segment) => Command::SetConstructionKept {
                sheet: sid,
                segment,
                kept: rng.chance(75),
            },
            None => fallback(rng),
        },
        45..=47 => {
            let (x, y) = (rng.val(-200.0, 100.0), rng.val(-200.0, 200.0));
            Command::AddCurve {
                sheet: sid,
                start: PointRef::New(PointMm::new(x, y)),
                c1: PointMm::new(x + 40.0, y + rng.val(20.0, 90.0)),
                c2: PointMm::new(x + 150.0, y + rng.val(20.0, 90.0)),
                end: PointRef::New(PointMm::new(x + rng.val(150.0, 200.0), y)),
            }
        }
        48..=54 => match rng.pick(&curves) {
            Some(curve) => {
                let (start, end) = match sheet.entity(curve).map(|e| e.kind.clone()) {
                    Ok(EntityKind::Curve { start, end, .. }) => (start, end),
                    _ => return fallback(rng),
                };
                if rng.chance(12) {
                    return Command::SetCurveLengthConstraint {
                        sheet: sid,
                        curve,
                        spec: None,
                    };
                }
                let current = sheet.curve_developed_mm(curve).unwrap_or(200.0);
                let point = if rng.chance(50) { start } else { end };
                let adjust = if rng.chance(70) {
                    LengthAdjust::Slide {
                        point,
                        angle_deg: [0.0, 45.0, 90.0][rng.below(3) as usize],
                    }
                } else {
                    LengthAdjust::Handle { point }
                };
                let reference = if rng.chance(55) {
                    let others: Vec<EntityId> =
                        curves.iter().copied().filter(|e| *e != curve).collect();
                    rng.pick(&others).map(|entity| LengthRef {
                        entity,
                        offset_mm: rng.val(-20.0, 30.0),
                    })
                } else {
                    None
                };
                Command::SetCurveLengthConstraint {
                    sheet: sid,
                    curve,
                    spec: Some(LengthSpec {
                        length_mm: current * (0.9 + rng.below(40) as f64 / 100.0),
                        adjust,
                        reference,
                    }),
                }
            }
            None => fallback(rng),
        },
        55..=57 => Command::AddRectangle {
            sheet: sid,
            corner: PointRef::New(PointMm::new(rng.val(-300.0, 200.0), rng.val(-300.0, 200.0))),
            width_mm: rng.val(40.0, 120.0),
            height_mm: rng.val(30.0, 90.0),
        },
        58..=60 => match rng.pick(&entities) {
            Some(entity) => Command::AddNotch {
                sheet: sid,
                entity,
                distance_mm: rng.val(1.0, 25.0),
                side: side(rng),
                length_mm: 5.0,
                style: NotchStyle::Single,
            },
            None => fallback(rng),
        },
        61..=62 => match (rng.pick(&points), rng.pick(&points)) {
            (Some(from), Some(to)) if from != to => Command::AddDimension {
                sheet: sid,
                from,
                to,
                offset_mm: 10.0,
            },
            _ => fallback(rng),
        },
        63 => match rng.pick(&entities) {
            Some(entity) => Command::AddFullDimension {
                sheet: sid,
                kind: FullDimensionKind::PathLength {
                    entity,
                    offset_mm: 8.0,
                },
                label: None,
                text_shift_mm: 0.0,
            },
            None => fallback(rng),
        },
        64..=66 => match rng.pick(&pieces_here) {
            Some(piece) => {
                let kind_roll = rng.below(3);
                let (target, kind) = match kind_roll {
                    0 => match rng.pick(&entities) {
                        Some(entity) => (InternalTarget::Path { entity }, InternalKind::Other),
                        None => return fallback(rng),
                    },
                    1 => match rng.pick(&points) {
                        Some(point) => (InternalTarget::Point { point }, InternalKind::Mark),
                        None => return fallback(rng),
                    },
                    _ => match rng.pick(&sheet.notches) {
                        Some(n) => (InternalTarget::Notch { notch: n.id }, InternalKind::Mark),
                        None => return fallback(rng),
                    },
                };
                Command::AddPieceInternal {
                    piece: piece.id,
                    target,
                    kind,
                }
            }
            None => fallback(rng),
        },
        67..=68 => match (rng.pick(&pieces_here), rng.pick(&points)) {
            (Some(piece), Some(point)) => Command::AddPieceMarking {
                piece: piece.id,
                at: MarkingAnchor::Point { point },
                shape: MarkingShape::Punch,
            },
            _ => fallback(rng),
        },
        69 if rng.chance(40) => match rng.pick(&pieces_here) {
            Some(piece) => match rng.pick(&piece.outline) {
                Some(edge) => Command::SetPieceFold {
                    piece: piece.id,
                    edge: edge.entity,
                    on: rng.chance(80),
                },
                None => fallback(rng),
            },
            None => fallback(rng),
        },
        69 => match (rng.pick(&pieces_here), rng.pick(&points), rng.pick(&points)) {
            (Some(piece), Some(from), Some(to)) if from != to => Command::AddButtonGroup {
                piece: piece.id,
                from,
                to,
                count: 2,
                element: ButtonElement::Button { diameter_mm: 10.0 },
                edge_margin_mm: 2.0,
            },
            _ => fallback(rng),
        },
        70 => match rng.pick(&pieces_here) {
            Some(piece) => match piece_inside(sheet, piece) {
                Some((c, half)) => Command::AddPieceDart {
                    piece: piece.id,
                    apex: PointRef::New(c),
                    toward: PointRef::New(PointMm::new(c.x, c.y - 10.0)),
                    length_mm: half,
                    opening_mm: 8.0,
                    fixed: Side::Left,
                },
                None => fallback(rng),
            },
            None => fallback(rng),
        },
        71 => match rng.pick(&pieces_here) {
            Some(piece) => Command::CreateDerivedPiece {
                mother: piece.id,
                kind: DerivedKind::Facing,
                operation: DerivedOp::Offset { distance_mm: -3.0 },
                name: None,
            },
            None => fallback(rng),
        },
        72 => match rng.pick(&pieces_here) {
            Some(piece) => Command::AddTopstitch {
                sheet: sid,
                source: TopstitchSource::Piece { piece: piece.id },
                distance_mm: 4.0,
                side: TopstitchSide::Inside,
            },
            None => fallback(rng),
        },
        73 => {
            let (a, b) = (rng.pick(&p.pieces), rng.pick(&p.pieces));
            match (a, b) {
                (Some(a), Some(b)) if a.id != b.id => {
                    match (a.outline.first(), b.outline.first()) {
                        (Some(ea), Some(eb)) => Command::AddSeamPair {
                            a: SeamSide {
                                piece: a.id,
                                edges: vec![ea.entity],
                            },
                            b: SeamSide {
                                piece: b.id,
                                edges: vec![eb.entity],
                            },
                            embu_mm: None,
                            tolerance_mm: None,
                        },
                        _ => fallback(rng),
                    }
                }
                _ => fallback(rng),
            }
        }
        74..=76 => match rng.pick(&points) {
            Some(point) => {
                let to = match sheet.position(point) {
                    Ok(at) if rng.chance(70) => {
                        PointMm::new(at.x + rng.val(-25.0, 25.0), at.y + rng.val(-25.0, 25.0))
                    }
                    _ => free(rng),
                };
                Command::MovePoint {
                    sheet: sid,
                    point,
                    to,
                }
            }
            None => fallback(rng),
        },
        77..=90 => {
            let objs = random_selection(rng, sheet);
            match Project::deletion_command(sid, &objs) {
                Some(c) => c,
                None => fallback(rng),
            }
        }
        _ => {
            let layers: Vec<LayerId> = sheet.layers.iter().map(|l| l.id).collect();
            match (rng.below(3), rng.pick(&layers)) {
                (0, _) if layers.len() < 2 => Command::AddLayer {
                    sheet: sid,
                    name: format!("Calque {}", layers.len() + 1),
                },
                (1, Some(layer)) => {
                    let pts: Vec<PointId> =
                        points.iter().copied().filter(|_| rng.chance(30)).collect();
                    let ents: Vec<EntityId> = entities
                        .iter()
                        .copied()
                        .filter(|_| rng.chance(30))
                        .collect();
                    Command::SetObjectsLayer {
                        sheet: sid,
                        points: pts,
                        entities: ents,
                        layer: Some(layer),
                    }
                }
                (_, Some(layer)) => Command::SetLayerLocked {
                    sheet: sid,
                    layer,
                    locked: rng.chance(55),
                },
                _ => Command::AddLayer {
                    sheet: sid,
                    name: "Calque 1".into(),
                },
            }
        }
    }
}

/// 1 à 3 objets de la feuille (points, tracés, crans, cotations).
fn random_selection(rng: &mut Rng, sheet: &Sheet) -> Vec<O> {
    let mut pool: Vec<O> = Vec::new();
    pool.extend(sheet.points.iter().map(|x| O::Point(x.id)));
    pool.extend(sheet.entities.iter().map(|x| O::Entity(x.id)));
    pool.extend(sheet.notches.iter().map(|x| O::Notch(x.id)));
    pool.extend(sheet.dimensions.iter().map(|x| O::Dimension(x.id)));
    let mut out = Vec::new();
    for _ in 0..1 + rng.below(3) {
        if let Some(o) = rng.pick(&pool) {
            if !out.contains(&o) {
                out.push(o);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------------------------

fn check_graph_and_trees(label: &str, p: &Project, rng: &mut Rng, cov: &mut Cov) {
    let before = bytes(p);
    let g = graph(p);
    let core_edges: BTreeSet<OEdge> = p.dependency_edges().iter().map(core_edge).collect();
    assert_eq!(core_edges, g.edges, "{label} : graphe");
    assert_eq!(
        p.dependency_edges(),
        p.dependency_edges(),
        "{label} : deux appels"
    );
    assert_eq!(bytes(p), before, "{label} : le graphe a modifié le projet");
    cov.max_edges = cov.max_edges.max(g.edges.len());
    for (_, _, n) in &g.edges {
        cov.natures.insert(n);
    }
    // Quelques objets au hasard (existants ou non).
    let names: Vec<String> = g.nodes.keys().cloned().collect();
    let mut chosen: Vec<String> = (0..4).filter_map(|_| rng.pick(&names)).collect();
    // Les objets les plus connectés valent mieux qu'un tirage uniforme.
    let mut degree: HashMap<&String, usize> = HashMap::new();
    for (a, b, _) in &g.edges {
        *degree.entry(a).or_default() += 1;
        *degree.entry(b).or_default() += 1;
    }
    if let Some((top, _)) = degree.iter().max_by_key(|(n, d)| (**d, n.to_string())) {
        chosen.push((*top).clone());
    }
    let objects: BTreeMap<String, O> = all_objects(p).into_iter().map(|o| (node(&o), o)).collect();
    assert_eq!(
        objects.keys().cloned().collect::<Vec<_>>(),
        g.nodes.keys().cloned().collect::<Vec<_>>(),
        "{label} : nœuds"
    );
    for name in chosen {
        let object = &objects[&name];
        for max in [None, Some(1), Some(2)] {
            let tree = p
                .dependency_tree(object, max)
                .unwrap_or_else(|| panic!("{label} : pas d'arbre pour {name}"));
            let again = p.dependency_tree(object, max).unwrap();
            assert_eq!(tree, again, "{label} : deux appels {name}");
            assert_eq!(
                tree.sheet.map(|s| s.0),
                g.nodes[&name],
                "{label} : feuille de {name}"
            );
            for (entries, up) in [(&tree.parents, true), (&tree.children, false)] {
                let expected = oracle_walk(&g, &name, up, max);
                let got: BTreeMap<String, usize> =
                    entries.iter().map(|e| (node(&e.object), e.depth)).collect();
                assert_eq!(
                    entries.len(),
                    got.len(),
                    "{label} : doublon dans l'arbre de {name}"
                );
                assert_eq!(
                    got,
                    expected.iter().map(|(k, v)| (k.clone(), v.0)).collect(),
                    "{label} : arbre de {name} (up {up}, profondeur {max:?})"
                );
                for e in entries.iter() {
                    assert_ne!(node(&e.object), name, "{label} : l'objet se cite lui-même");
                    assert_eq!(
                        e.sheet.map(|s| s.0),
                        g.nodes[&node(&e.object)],
                        "{label} : feuille"
                    );
                    // Profondeur croissante, et via = un objet de la profondeur précédente.
                    assert!(
                        e.depth == 1 && node(&e.via) == name
                            || e.depth > 1
                                && entries
                                    .iter()
                                    .any(|x| x.object == e.via && x.depth == e.depth - 1),
                        "{label} : via de {}",
                        node(&e.object)
                    );
                    // L'arête via -> objet existe avec cette nature.
                    let (a, b) = if up {
                        (node(&e.object), node(&e.via))
                    } else {
                        (node(&e.via), node(&e.object))
                    };
                    assert!(
                        g.edges.contains(&(a, b, nature(e.nature))),
                        "{label} : arête de l'arbre absente de l'oracle"
                    );
                    if e.depth == 1 {
                        assert_eq!(
                            Some(nature(e.nature)),
                            expected[&node(&e.object)].1,
                            "{label} : nature directe de {}",
                            node(&e.object)
                        );
                    }
                    cov.max_depth = cov.max_depth.max(e.depth);
                }
                let order: Vec<(usize, O)> = entries
                    .iter()
                    .map(|e| (e.depth, e.object.clone()))
                    .collect();
                let mut sorted = order.clone();
                sorted.sort();
                assert_eq!(order, sorted, "{label} : tri");
            }
            cov.trees += 1;
        }
    }
    assert_eq!(bytes(p), before, "{label} : l'arbre a modifié le projet");
    // Objet inexistant.
    assert!(p
        .dependency_tree(&O::Point(PointId(u64::MAX)), None)
        .is_none());
}

fn all_objects(p: &Project) -> Vec<O> {
    let mut out = Vec::new();
    for s in &p.sheets {
        out.extend(s.points.iter().map(|x| O::Point(x.id)));
        out.extend(s.entities.iter().map(|x| O::Entity(x.id)));
        out.extend(s.guides.iter().map(|x| O::Guide(x.id)));
        out.extend(s.notches.iter().map(|x| O::Notch(x.id)));
        out.extend(s.dimensions.iter().map(|x| O::Dimension(x.id)));
        out.extend(s.full_dimensions.iter().map(|x| O::FullDimension(x.id)));
        out.extend(s.topstitches.iter().map(|x| O::Topstitch(x.id)));
    }
    out.extend(p.pieces.iter().map(|x| O::Piece(x.id)));
    out.extend(p.seams.iter().map(|x| O::SeamPair(x.id)));
    out.extend(p.variables.iter().map(|x| O::Variable(x.name.clone())));
    out
}

/// Points libres, extrémités des tracés demandés (lus dans le JSON d'avant).
fn free_ends(before: &Value, requested: &[O]) -> BTreeSet<String> {
    let mut free: HashSet<u64> = HashSet::new();
    let mut ends: HashMap<u64, Vec<u64>> = HashMap::new();
    for s in arr(&before["sheets"]) {
        for p in arr(&s["points"]) {
            if p["definition"]["type"] == "free" {
                free.insert(num(&p["id"]));
            }
        }
        for e in arr(&s["entities"]) {
            let k = &e["kind"];
            let list: Vec<&Value> = match k["type"].as_str().unwrap() {
                "segment" => vec![&k["from"], &k["to"]],
                "curve" => vec![&k["start"], &k["end"]],
                "circle" => vec![&k["center"]],
                "arc" => vec![&k["center"], &k["start"], &k["end"]],
                _ => arr(&k["nodes"]).map(|n| &n["point"]).collect(),
            };
            ends.insert(num(&e["id"]), list.into_iter().map(num).collect());
        }
    }
    let mut out = BTreeSet::new();
    for o in requested {
        if let O::Entity(e) = o {
            for q in ends.get(&e.0).into_iter().flatten() {
                if free.contains(q) {
                    out.insert(format!("point:{q}"));
                }
            }
        }
    }
    out
}

fn check_impact(label: &str, p: &Project, rng: &mut Rng, cov: &mut Cov) {
    let sheet = &p.sheets[rng.below(p.sheets.len() as u64) as usize];
    let sid = sheet.id;
    let selection = random_selection(rng, sheet);
    if selection.is_empty() {
        return;
    }
    let before_bytes = bytes(p);
    let impact = p.deletion_impact(sid, &selection);
    assert_eq!(
        impact,
        p.deletion_impact(sid, &selection),
        "{label} : deux appels"
    );
    assert_eq!(
        bytes(p),
        before_bytes,
        "{label} : l'impact a modifié le projet"
    );
    cov.impacts += 1;
    cov.impact_multi += u64::from(selection.len() > 1);
    let command = Project::deletion_command(sid, &selection).expect("sélection non vide");
    let mut after = p.clone();
    match after.apply(command.clone()) {
        Err(e) => {
            assert_eq!(
                impact.refused.as_deref(),
                Some(e.to_string().as_str()),
                "{label} : refus annoncé != refus réel"
            );
            assert_eq!(bytes(&after), before_bytes, "{label} : refus avec trace");
            cov.impact_refused += 1;
            return;
        }
        Ok(outcome) => {
            assert_eq!(impact.refused, None, "{label} : refus annoncé à tort");
            assert_eq!(impact.notes, outcome.notes, "{label} : remarques");
        }
    }
    // Différence lue dans les JSON, avec l'oracle de graphe.
    let (gb, ga) = (graph(p), graph(&after));
    let requested: BTreeSet<String> = selection.iter().map(node).collect();
    let before_json = serde_json::to_value(p).unwrap();
    let ends = free_ends(&before_json, &selection);
    let gone: BTreeSet<String> = gb
        .nodes
        .keys()
        .filter(|n| !ga.nodes.contains_key(*n))
        .cloned()
        .collect();
    assert!(
        requested.iter().all(|r| gone.contains(r)),
        "{label} : un objet demandé survit : {requested:?} / {gone:?}"
    );
    let expected_cleaned: BTreeSet<String> = gone
        .iter()
        .filter(|n| !requested.contains(*n) && ends.contains(*n))
        .cloned()
        .collect();
    let expected_removed: BTreeSet<String> = gone
        .iter()
        .filter(|n| !requested.contains(*n) && !ends.contains(*n))
        .cloned()
        .collect();
    let removed: BTreeSet<String> = impact.removed.iter().map(node).collect();
    let cleaned: BTreeSet<String> = impact.cleaned.iter().map(node).collect();
    assert_eq!(removed, expected_removed, "{label} : supprimés");
    assert_eq!(cleaned, expected_cleaned, "{label} : nettoyés");
    assert_eq!(removed.len(), impact.removed.len(), "{label} : doublon");
    let expected_unlinked: BTreeSet<OEdge> = gb
        .edges
        .iter()
        .filter(|e| !ga.edges.contains(*e) && ga.nodes.contains_key(&e.1))
        .cloned()
        .collect();
    let unlinked: BTreeSet<OEdge> = impact.unlinked.iter().map(core_edge).collect();
    assert_eq!(unlinked, expected_unlinked, "{label} : liens retirés");
    assert_eq!(unlinked.len(), impact.unlinked.len(), "{label} : doublon");
    assert_eq!(
        impact.is_trivial(),
        expected_removed.is_empty() && expected_unlinked.is_empty(),
        "{label} : trivial"
    );
    // Les liens retirés vont dans le même sens que ce que décrit l'enfant conservé.
    for e in &impact.unlinked {
        assert!(ga.nodes.contains_key(&node(&e.child)), "{label}");
    }
    cov.impact_trivial += u64::from(impact.is_trivial());
    cov.impact_removed += u64::from(!impact.removed.is_empty());
    cov.impact_cleaned += u64::from(!impact.cleaned.is_empty());
    cov.impact_unlinked += u64::from(!impact.unlinked.is_empty());
    cov.impact_notes += u64::from(!impact.notes.is_empty());
    // Exécution réelle dans un document : une seule étape, annulation exacte, relecture identique.
    let mut doc = Document::new(p.clone());
    doc.execute(command).unwrap();
    assert_eq!(bytes(doc.project()), bytes(&after), "{label} : exécution");
    assert!(doc.undo(), "{label}");
    assert_eq!(
        bytes(doc.project()),
        before_bytes,
        "{label} : annulation en une étape"
    );
    let reread = persist::from_json(&before_bytes).unwrap();
    assert_eq!(
        reread.deletion_impact(sid, &selection),
        impact,
        "{label} : relecture"
    );
}

fn run(seed: u64, cov: &mut Cov) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut doc = Document::new(Project::new("Adverse dépendances"));
    let mut next_var = 0u64;
    if seed % 2 == 0 {
        doc.execute(Command::CreateSheet {
            name: "Deux".into(),
        })
        .unwrap();
    }
    for step in 0..STEPS {
        let label = format!("graine {seed}, pas {step}");
        cov.steps += 1;
        let before = bytes(doc.project());
        let roll = rng.below(100);
        if roll < 5 {
            if doc.undo() {
                cov.undone += 1;
            }
        } else if roll < 12 {
            if doc.redo() {
                cov.redone += 1;
            }
        } else {
            let cmd = random_command(&mut rng, doc.project(), &mut next_var);
            let rect = match &cmd {
                Command::AddRectangle {
                    sheet,
                    corner: PointRef::New(c),
                    width_mm,
                    height_mm,
                } => Some((
                    *sheet,
                    PointMm::new(c.x + width_mm / 2.0, c.y + height_mm / 2.0),
                )),
                _ => None,
            };
            match doc.execute(cmd) {
                Ok(_) => {
                    cov.accepted += 1;
                    if let Some((sheet, at)) = rect {
                        if rng.chance(85) {
                            let name = format!("Pièce {step}");
                            let extracted = doc.execute(Command::ExtractPiece {
                                sheet,
                                source: PieceSource::Click { at },
                                name,
                                detached: false,
                            });
                            cov.pieces += u64::from(extracted.is_ok());
                        }
                    }
                }
                Err(_) => {
                    cov.refused += 1;
                    assert_eq!(bytes(doc.project()), before, "{label} : refus avec trace");
                }
            }
        }
        let p = doc.project().clone();
        check_graph_and_trees(&label, &p, &mut rng, cov);
        check_impact(&label, &p, &mut rng, cov);
        if step % 7 == 6 {
            let again = persist::from_json(&bytes(&p)).unwrap();
            assert_eq!(
                again.dependency_edges(),
                p.dependency_edges(),
                "{label} : relecture"
            );
        }
    }
}

#[test]
fn campagne_adverse_dependances() {
    let mut cov = Cov::default();
    for seed in 1..=SEQUENCES {
        run(seed, &mut cov);
    }
    eprintln!(
        "couverture : acceptées {}, refusées {}, annulées {}, rétablies {}, pièces {}, arbres {}, \
         profondeur max {}, arêtes max {}, impacts {} (refusés {}, triviaux {}, supprimés {}, \
         nettoyés {}, liens retirés {}, remarques {}, multiples {}), natures {:?}",
        cov.accepted,
        cov.refused,
        cov.undone,
        cov.redone,
        cov.pieces,
        cov.trees,
        cov.max_depth,
        cov.max_edges,
        cov.impacts,
        cov.impact_refused,
        cov.impact_trivial,
        cov.impact_removed,
        cov.impact_cleaned,
        cov.impact_unlinked,
        cov.impact_notes,
        cov.impact_multi,
        cov.natures
    );
    assert_eq!(cov.steps, SEQUENCES * STEPS as u64);
    assert!(
        cov.accepted > 2000 && cov.refused > 200,
        "{:?}",
        (cov.accepted, cov.refused)
    );
    assert!(
        cov.undone > 100 && cov.redone > 20,
        "{:?}",
        (cov.undone, cov.redone)
    );
    assert!(cov.pieces > 100, "pièces : {}", cov.pieces);
    assert!(cov.max_depth >= 4, "profondeur : {}", cov.max_depth);
    assert!(cov.impact_refused > 100, "refusés : {}", cov.impact_refused);
    assert!(
        cov.impact_trivial > 500,
        "triviaux : {}",
        cov.impact_trivial
    );
    assert!(
        cov.impact_removed > 100,
        "supprimés : {}",
        cov.impact_removed
    );
    assert!(
        cov.impact_cleaned > 100,
        "nettoyés : {}",
        cov.impact_cleaned
    );
    assert!(cov.impact_unlinked > 200, "liens : {}", cov.impact_unlinked);
    assert!(cov.impact_notes > 100, "remarques : {}", cov.impact_notes);
    assert!(cov.impact_multi > 500, "multiples : {}", cov.impact_multi);
    // Chaque nature du graphe est exercée au moins une fois par la campagne.
    for n in [
        "construction",
        "path_point",
        "kept",
        "formula_ref",
        "variable_ref",
        "imposed_length",
        "length_equality",
        "notch_ref",
        "dimension_ref",
        "piece_outline",
        "piece_internal",
        "piece_marking",
        "piece_fold",
        "piece_dart",
        "piece_buttons",
        "piece_derived",
        "seam_pair",
        "topstitch_source",
        "topstitch_line",
    ] {
        assert!(cov.natures.contains(n), "nature jamais exercée : {n}");
    }
    let _ = Side::Left;
}
