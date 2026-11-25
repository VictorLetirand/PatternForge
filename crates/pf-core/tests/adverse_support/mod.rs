//! Aides partagées des campagnes adverses (US-17.6 et suivantes).

#![allow(dead_code)]

use pf_core::clipboard::Fragment;
use pf_core::command::{Command, CurveHandle, DivisionSpec, PointRef, TangentAt};
use pf_core::model::*;
use pf_core::{contour, persist, Document, Project};
use pf_geometry::{PointMm, Side, Transform};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// Outils
// ---------------------------------------------------------------------------

pub fn snap(p: &Project) -> String {
    serde_json::to_string(p).expect("sérialisation brute")
}

/// Projet riche : feuilles, courbe, points dérivés, crans, cotations, pièce,
/// calque, groupe.
pub fn rich_project() -> Project {
    let mut p = pf_core::demo_piece_project().unwrap();
    let sheet = p.sheets[0].id;
    let rect: Vec<EntityId> = p.sheets[0].entities.iter().map(|e| e.id).take(4).collect();
    let pts: Vec<PointId> = p.sheets[0].points.iter().map(|q| q.id).collect();
    let layer = p
        .apply(Command::AddLayer {
            sheet,
            name: "Calque A".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet,
        points: vec![pts[0]],
        entities: vec![],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet,
        entities: vec![rect[0], rect[1]],
    })
    .unwrap();
    let curve = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(PointMm::new(10.0, 10.0)),
            c1: PointMm::new(30.0, 60.0),
            c2: PointMm::new(60.0, 60.0),
            end: PointRef::New(PointMm::new(80.0, 10.0)),
        })
        .unwrap();
    p.apply(Command::AddPointAt {
        sheet,
        point: PointRef::Derived(PointDefinition::OnCurve {
            curve: curve.entities[0],
            t: 0.4,
        }),
    })
    .unwrap();
    p.apply(Command::AddPointAt {
        sheet,
        point: PointRef::Derived(PointDefinition::OnSegmentRatio {
            segment: rect[0],
            ratio: 0.5,
        }),
    })
    .unwrap();
    p.apply(Command::SetEntityName {
        sheet,
        entity: rect[2],
        name: Some("Côté".into()),
    })
    .unwrap();
    p.apply(Command::CreateSheet {
        name: "Seconde".into(),
    })
    .unwrap();
    p.validate().unwrap();
    p
}

/// Projet riche + US-02.1 : cercle, arc, point d'intersection et cran sur l'arc.
pub fn rich_project_with_arcs() -> Project {
    let mut p = rich_project();
    let sheet = p.sheets[0].id;
    let c = p
        .apply(Command::AddCircle {
            sheet,
            center: PointRef::New(PointMm::new(40.0, 40.0)),
            radius_mm: 25.0,
        })
        .unwrap()
        .entities[0];
    let a = p
        .apply(Command::AddArc {
            sheet,
            center: PointRef::New(PointMm::new(90.0, 40.0)),
            radius_mm: 25.0,
            start_deg: 20.0,
            end_deg: 200.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    p.apply(Command::AddPointAt {
        sheet,
        point: PointRef::Derived(PointDefinition::CircleIntersection {
            first: c,
            second: a,
            near: PointMm::new(65.0, 60.0).into(),
        }),
    })
    .unwrap();
    p.apply(Command::AddNotch {
        sheet,
        entity: a,
        distance_mm: 20.0,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    p.validate().unwrap();
    p
}

/// Projet riche + courbes par points de passage (US-02.2) : une courbe avec
/// point anguleux et tangentes saisies, un cran, une Bézier convertie.
pub fn rich_project_with_splines() -> Project {
    let mut p = rich_project_with_arcs();
    let sheet = p.sheets[0].id;
    let sp = p
        .apply(Command::AddSpline {
            sheet,
            points: [(-60.0, -60.0), (-30.0, -20.0), (0.0, -50.0), (40.0, -70.0)]
                .iter()
                .map(|&(x, y)| PointRef::New(PointMm::new(x, y)))
                .collect(),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetSplineNode {
        sheet,
        spline: sp,
        index: 2,
        corner: true,
        tangent: Some(OffsetMm { dx: 20.0, dy: 5.0 }),
        tangent_in: None,
    })
    .unwrap();
    p.apply(Command::SetSplineNode {
        sheet,
        spline: sp,
        index: 1,
        corner: false,
        tangent: Some(OffsetMm { dx: 25.0, dy: 10.0 }),
        tangent_in: None,
    })
    .unwrap();
    p.apply(Command::AddNotch {
        sheet,
        entity: sp,
        distance_mm: 30.0,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let c = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(PointMm::new(100.0, -60.0)),
            c1: PointMm::new(110.0, -20.0),
            c2: PointMm::new(150.0, -90.0),
            end: PointRef::New(PointMm::new(160.0, -50.0)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::CurveToSpline { sheet, curve: c }).unwrap();
    p.validate().unwrap();
    p
}

#[derive(Clone)]
pub enum Key {
    Idx(usize),
    Name(String),
}

pub fn walk(v: &Value, path: &mut Vec<Key>, out: &mut Vec<Vec<Key>>) {
    match v {
        Value::Object(m) => {
            for (k, c) in m {
                path.push(Key::Name(k.clone()));
                out.push(path.clone());
                walk(c, path, out);
                path.pop();
            }
        }
        Value::Array(a) => {
            for (i, c) in a.iter().enumerate() {
                path.push(Key::Idx(i));
                out.push(path.clone());
                walk(c, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

pub fn get_mut<'a>(v: &'a mut Value, path: &[Key]) -> Option<&'a mut Value> {
    let mut cur = v;
    for k in path {
        cur = match (k, cur) {
            (Key::Idx(i), Value::Array(a)) => a.get_mut(*i)?,
            (Key::Name(n), Value::Object(m)) => m.get_mut(n)?,
            _ => return None,
        };
    }
    Some(cur)
}

pub fn get<'a>(v: &'a Value, path: &[Key]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = match (k, cur) {
            (Key::Idx(i), Value::Array(a)) => a.get(*i)?,
            (Key::Name(n), Value::Object(m)) => m.get(n)?,
            _ => return None,
        };
    }
    Some(cur)
}

pub fn big_float(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}

/// Contrôle commun d'une ouverture : jamais de panic (vérifié par l'appelant),
/// et un projet accepté est valide et se réécrit/relit à l'identique.
pub fn check_open(text: &str) -> Result<Project, String> {
    match persist::from_json(text) {
        Err(e) => {
            if e.to_string().trim().is_empty() {
                return Err("message d'erreur vide".into());
            }
            Ok(Project::new("erreur-propre"))
        }
        Ok(p) => {
            p.validate()
                .map_err(|e| format!("accepté mais invalide : {e}"))?;
            let a = persist::to_json(&p).map_err(|e| format!("non réécrivable : {e}"))?;
            let q = persist::from_json(&a).map_err(|e| format!("relecture : {e}"))?;
            let b = persist::to_json(&q).map_err(|e| format!("réécriture : {e}"))?;
            if a != b || p != q {
                return Err("réécriture non idempotente".into());
            }
            Ok(p)
        }
    }
}

pub fn run_mutations(base: &Value, label: &str) -> (usize, usize) {
    let mut paths = Vec::new();
    walk(base, &mut Vec::new(), &mut paths);
    let mut failures: Vec<String> = Vec::new();
    let (mut ok, mut refused) = (0usize, 0usize);
    let replacements: Vec<(&str, Value)> = vec![
        ("null", Value::Null),
        ("chaine", json!("x")),
        ("neg", json!(-1)),
        ("zero", json!(0)),
        ("1e308", big_float(1e308)),
        ("u64max", json!(u64::MAX)),
        ("id mort", json!(99_999_999u64)),
        ("bool", json!(true)),
        ("tableau", json!([])),
        ("objet", json!({})),
        ("petit", big_float(1e-300)),
    ];
    for path in &paths {
        let mut variants: Vec<(String, Value)> = Vec::new();
        for (name, rep) in &replacements {
            let mut v = base.clone();
            *get_mut(&mut v, path).unwrap() = rep.clone();
            variants.push((format!("remplace par {name}"), v));
        }
        // suppression
        {
            let mut v = base.clone();
            let (last, parent) = path.split_last().unwrap();
            match (last, get_mut(&mut v, parent).unwrap()) {
                (Key::Name(n), Value::Object(m)) => {
                    m.remove(n);
                }
                (Key::Idx(i), Value::Array(a)) => {
                    a.remove(*i);
                }
                _ => {}
            }
            variants.push(("supprime".into(), v));
        }
        // duplication d'un élément de tableau
        if let Some(Key::Idx(i)) = path.last() {
            let mut v = base.clone();
            if let Value::Array(a) = get_mut(&mut v, &path[..path.len() - 1]).unwrap() {
                let c = a[*i].clone();
                a.insert(*i, c);
            }
            variants.push(("duplique".into(), v));
        }
        for (what, v) in variants {
            let text = serde_json::to_string(&v).unwrap();
            let res = catch_unwind(AssertUnwindSafe(|| check_open(&text)));
            match res {
                Err(_) => failures.push(format!("{label}: PANIC ({what}) en {}", fmt_path(path))),
                Ok(Err(msg)) => {
                    failures.push(format!("{label}: {msg} ({what}) en {}", fmt_path(path)))
                }
                Ok(Ok(_)) => {
                    if let Ok(proj) = persist::from_json(&text) {
                        // un fichier accepté doit aussi s'exporter sans panic ni NaN
                        let r = catch_unwind(AssertUnwindSafe(|| {
                            check_export(&proj, "mutation");
                        }));
                        if r.is_err() {
                            failures.push(format!(
                                "{label}: export en échec ({what}) en {}",
                                fmt_path(path)
                            ));
                        }
                        ok += 1
                    } else {
                        refused += 1
                    }
                }
            }
        }
        // clé inconnue ajoutée sur un objet : refusée, ou conservée à la réécriture
        if matches!(get(base, path), Some(Value::Object(_))) {
            let mut v = base.clone();
            if let Some(Value::Object(m)) = get_mut(&mut v, path) {
                m.insert("zz_inconnu".into(), json!(7));
            }
            let text = serde_json::to_string(&v).unwrap();
            if let Ok(p) = persist::from_json(&text) {
                let out: Value = serde_json::from_str(&persist::to_json(&p).unwrap()).unwrap();
                let mut q = path.to_vec();
                q.push(Key::Name("zz_inconnu".into()));
                if get(&out, &q) != Some(&json!(7)) {
                    failures.push(format!(
                        "{label}: PERTE SILENCIEUSE de la clé inconnue en {}",
                        fmt_path(path)
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} défaut(s) :\n{}",
        failures.len(),
        failures
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    (ok, refused)
}

pub fn fmt_path(p: &[Key]) -> String {
    p.iter()
        .map(|k| match k {
            Key::Idx(i) => format!("[{i}]"),
            Key::Name(n) => format!(".{n}"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 2. Valeurs extrêmes et géométries dégénérées
// ---------------------------------------------------------------------------

pub fn extreme_numbers() -> Vec<f64> {
    vec![
        0.0,
        -0.0,
        1e-300,
        -1e-300,
        f64::MIN_POSITIVE,
        f64::EPSILON,
        1e-9,
        1.0,
        -1.0,
        360.0,
        1e6,
        1e9,
        1e15,
        1e100,
        1e300,
        1e308,
        -1e308,
        f64::MAX,
        f64::MIN,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ]
}

/// Applique `cmd` : jamais de panic, projet intact si refusé, valide si accepté.
pub fn try_cmd(p: &mut Project, cmd: Command, what: &str) -> bool {
    let before = snap(p);
    let r = catch_unwind(AssertUnwindSafe(|| p.apply(cmd)))
        .unwrap_or_else(|_| panic!("PANIC : {what}"));
    match r {
        Ok(_) => {
            p.validate()
                .unwrap_or_else(|e| panic!("{what} : accepté mais invalide : {e}"));
            true
        }
        Err(e) => {
            assert!(!e.to_string().is_empty(), "{what} : message vide");
            assert_eq!(snap(p), before, "{what} : refusé mais projet modifié");
            false
        }
    }
}

pub fn base_square() -> (Project, SheetId, Vec<PointId>, Vec<EntityId>) {
    let mut p = Project::new("Extrême");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(PointMm::new(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 50.0,
        })
        .unwrap();
    (p, sheet, o.points, o.entities)
}

pub fn names() -> Vec<String> {
    vec![
        "".into(),
        " ".into(),
        "a".into(),
        "x".repeat(120),
        "x".repeat(121),
        "é".repeat(120),
        "é".repeat(121),
        "日本語の名前 ✂️ 縫製".into(),
        "👩‍👩‍👧‍👦".repeat(30),
        "👩‍👩‍👧‍👦".repeat(5),
        "e\u{301}".repeat(60),
        "\u{202e}rtl\u{202c}".into(),
        "\u{200b}".into(),
        "a\nb".into(),
        "a\tb".into(),
        "nul\u{0}char".into(),
        "\u{7f}".into(),
        "<script>&\"'</script>".into(),
        "../../etc/passwd".into(),
        "x".repeat(100_000),
        "\u{fffd}".into(),
        "𝔘𝔫𝔦𝔠𝔬𝔡𝔢".into(),
    ]
}

// ---------------------------------------------------------------------------
// 3. Séquences aléatoires de commandes
// ---------------------------------------------------------------------------

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub const BOGUS: u64 = 9_999_999;

pub fn pick<T: Copy>(r: &mut Rng, items: &[T], bogus: T) -> T {
    if items.is_empty() || r.chance(4) {
        bogus
    } else {
        items[r.below(items.len())]
    }
}

pub fn some<T: Copy>(r: &mut Rng, items: &[T], max: usize) -> Vec<T> {
    let n = r.below(max + 1);
    (0..n)
        .filter_map(|_| items.get(r.below(items.len())))
        .copied()
        .collect()
}

pub fn num(r: &mut Rng) -> f64 {
    match r.below(10) {
        0..=5 => (r.below(41) as f64 - 20.0) * 10.0,
        6 | 7 => (r.unit() * 400.0 - 200.0 * 1.0).round() / 10.0 * 10.0,
        8 => [0.0, 1.0, 45.0, 90.0, 0.5, -5.0, 1e-9][r.below(7)],
        _ => extreme_numbers()[r.below(extreme_numbers().len())],
    }
}

pub fn len_num(r: &mut Rng) -> f64 {
    match r.below(10) {
        0..=6 => 5.0 + r.below(150) as f64,
        7 => r.unit() * 100.0,
        8 => -(r.below(50) as f64),
        _ => extreme_numbers()[r.below(extreme_numbers().len())],
    }
}

pub fn pt_mm(r: &mut Rng) -> PointMm {
    if r.chance(85) {
        PointMm::new(
            r.below(21) as f64 * 10.0 - 100.0,
            r.below(21) as f64 * 10.0 - 100.0,
        )
    } else {
        PointMm::new(num(r), num(r))
    }
}

pub fn rname(r: &mut Rng) -> String {
    let pool = [
        "A",
        "Devant",
        "Dos (copie)",
        "Calque X",
        "Écharpe ✂",
        "",
        "  ",
        "bad\nname",
        "B",
    ];
    if r.chance(3) {
        names()[r.below(names().len() - 3)].clone()
    } else {
        pool[r.below(pool.len())].to_string()
    }
}

pub struct Ctx {
    pub donors: Vec<(Sheet, Vec<Piece>)>,
    pub bank: Vec<Fragment>,
    /// Campagnes US-02.1 : génère aussi cercles, arcs et intersections de cercles.
    /// Faux = flux aléatoire historique, inchangé.
    pub arcs: bool,
    /// Campagnes US-02.2 : génère aussi les commandes de courbes par points de passage.
    pub splines: bool,
    /// Campagnes US-02.3 : génère aussi congés, chanfreins et tangentes.
    pub tangency: bool,
    /// Campagnes US-02.4 : génère aussi des points d'intersection générale de tracés.
    pub traces: bool,
    /// Campagnes US-02.5 : génère aussi des points relatifs et leurs paramètres.
    pub relative: bool,
    /// Campagnes US-02.6 : génère aussi des divisions de tracés.
    pub divisions: bool,
    /// Campagnes US-02.7 : génère aussi prolonger, couper, fusionner, inverser.
    pub paths: bool,
    /// Campagnes US-02.8 : génère aussi les commandes de lignes d'aide.
    pub guides: bool,
    /// Campagnes US-02.10 : génère aussi des décalages de contour.
    pub offsets: bool,
    /// Campagnes US-02.11 : génère aussi des symétries liées et leur dépliage.
    pub mirrors: bool,
}

#[allow(clippy::type_complexity)]
pub fn ids(
    s: Option<&Sheet>,
) -> (
    Vec<PointId>,
    Vec<EntityId>,
    Vec<NotchId>,
    Vec<DimensionId>,
    Vec<LayerId>,
    Vec<GroupId>,
) {
    match s {
        Some(s) => (
            s.points.iter().map(|x| x.id).collect(),
            s.entities.iter().map(|x| x.id).collect(),
            s.notches.iter().map(|x| x.id).collect(),
            s.dimensions.iter().map(|x| x.id).collect(),
            s.layers.iter().map(|x| x.id).collect(),
            s.groups.iter().map(|x| x.id).collect(),
        ),
        None => Default::default(),
    }
}

pub fn side(r: &mut Rng) -> Side {
    if r.chance(50) {
        Side::Left
    } else {
        Side::Right
    }
}

pub fn gen(r: &mut Rng, p: &Project, ctx: &Ctx, depth: u32) -> Command {
    let sheet_ids: Vec<SheetId> = p.sheets.iter().map(|s| s.id).collect();
    let sheet = pick(r, &sheet_ids, SheetId(BOGUS));
    let s = p.sheet(sheet).ok();
    let (pts, ents, notches, dims, layers, groups) = ids(s);
    let pieces: Vec<PieceId> = p.pieces.iter().map(|x| x.id).collect();
    let pt = |r: &mut Rng| pick(r, &pts, PointId(BOGUS));
    let en = |r: &mut Rng| pick(r, &ents, EntityId(BOGUS));
    let derived_pts: Vec<PointId> = s
        .map(|s| {
            s.points
                .iter()
                .filter(|q| !q.definition.is_free())
                .map(|q| q.id)
                .collect()
        })
        .unwrap_or_default();
    let derived = |r: &mut Rng| {
        if r.chance(85) {
            pick(r, &derived_pts, PointId(BOGUS))
        } else {
            pick(r, &pts, PointId(BOGUS))
        }
    };
    let pref = |r: &mut Rng| -> PointRef {
        match r.below(10) {
            0..=3 => PointRef::New(pt_mm(r)),
            4..=7 => PointRef::Existing(pick(r, &pts, PointId(BOGUS))),
            8 => PointRef::Derived(match r.below(if ctx.arcs { 6 } else { 5 }) {
                0 => PointDefinition::Free {
                    x: pt_mm(r).x,
                    y: pt_mm(r).y,
                },
                1 => PointDefinition::OnSegment {
                    segment: pick(r, &ents, EntityId(BOGUS)),
                    distance_mm: len_num(r),
                    origin: if r.chance(50) {
                        SegmentEnd::Start
                    } else {
                        SegmentEnd::End
                    },
                },
                2 => PointDefinition::OnSegmentRatio {
                    segment: pick(r, &ents, EntityId(BOGUS)),
                    ratio: r.unit(),
                },
                3 => PointDefinition::Intersection {
                    first: pick(r, &ents, EntityId(BOGUS)),
                    second: pick(r, &ents, EntityId(BOGUS)),
                },
                4 => PointDefinition::OnCurve {
                    curve: pick(r, &ents, EntityId(BOGUS)),
                    t: r.unit(),
                },
                _ => PointDefinition::CircleIntersection {
                    first: pick(r, &ents, EntityId(BOGUS)),
                    second: pick(r, &ents, EntityId(BOGUS)),
                    near: pt_mm(r).into(),
                },
            }),
            _ => PointRef::New(PointMm::new(num(r), num(r))),
        }
    };
    let spline_ents: Vec<EntityId> = s
        .map(|s| {
            s.entities
                .iter()
                .filter(|e| matches!(e.kind, EntityKind::Spline { .. }))
                .map(|e| e.id)
                .collect()
        })
        .unwrap_or_default();
    let curve_ents: Vec<EntityId> = s
        .map(|s| {
            s.entities
                .iter()
                .filter(|e| matches!(e.kind, EntityKind::Curve { .. }))
                .map(|e| e.id)
                .collect()
        })
        .unwrap_or_default();
    let few = s.is_none_or(|s| s.entities.len() < 4);
    let round_ents: Vec<EntityId> = s
        .map(|s| {
            s.entities
                .iter()
                .filter(|e| !matches!(e.kind, EntityKind::Curve { .. } | EntityKind::Spline { .. }))
                .map(|e| e.id)
                .collect()
        })
        .unwrap_or_default();
    let seg_ents: Vec<EntityId> = s
        .map(|s| {
            s.entities
                .iter()
                .filter(|e| matches!(e.kind, EntityKind::Segment { .. }))
                .map(|e| e.id)
                .collect()
        })
        .unwrap_or_default();
    // Point visé : milieu d'un tracé (le plus souvent), sinon n'importe où.
    let aim_on = |r: &mut Rng, e: EntityId| -> PointMm {
        let sh = s.and_then(|s| s.shape(e).ok());
        match sh {
            Some(sh) if r.chance(85) => {
                let v = sh.sample(8);
                if v.len() == 2 {
                    PointMm::new((v[0].x + v[1].x) / 2.0, (v[0].y + v[1].y) / 2.0)
                } else {
                    v[v.len() / 2]
                }
            }
            _ => pt_mm(r),
        }
    };
    let blend_len = |r: &mut Rng| -> f64 {
        match r.below(10) {
            0..=6 => 1.0 + r.below(40) as f64,
            7 => r.unit() * 30.0,
            _ => len_num(r),
        }
    };
    let guide_ids: Vec<GuideId> = s
        .map(|s| s.guides.iter().map(|g| g.id).collect())
        .unwrap_or_default();
    let k = if ctx.mirrors && r.chance(22) {
        // US-02.11 : symétrie liée, dépliage
        89 + r.below(2)
    } else if ctx.offsets && r.chance(20) {
        // US-02.10 : décalage de contour
        88
    } else if ctx.guides && r.chance(25) {
        // US-02.8 : lignes d'aide (4 commandes)
        84 + r.below(4)
    } else if ctx.paths && r.chance(40) {
        // US-02.7 : prolonger, raccourcir, couper, fusionner, inverser (5 commandes)
        79 + r.below(5)
    } else if ctx.divisions && r.chance(30) {
        // US-02.6 : divisions de tracés
        78
    } else if ctx.relative && r.chance(40) {
        // US-02.5 : points relatifs (création, paramètres)
        76 + r.below(2)
    } else if ctx.traces && r.chance(35) {
        // US-02.4 : intersections générales (2 commandes)
        74 + r.below(2)
    } else if ctx.tangency && r.chance(45) {
        // US-02.3 : congé, chanfrein, tangentes (4 commandes)
        70 + r.below(4)
    } else if ctx.splines && r.chance(40) {
        // US-02.2 : courbes par points de passage (6 commandes)
        64 + r.below(6)
    } else if ctx.arcs && r.chance(35) {
        // US-02.1 : cercles et arcs (7 commandes)
        57 + r.below(7)
    } else if few && r.chance(70) {
        r.below(7)
    } else {
        r.below(57)
    };
    // poids supplémentaire des commandes rarement acceptées
    let k = match k {
        52 | 53 => 44,
        54..=56 => 15,
        k => k,
    };
    match k {
        0 => Command::AddSegment {
            sheet,
            from: pref(r),
            to: pref(r),
        },
        1 => Command::AddSegmentPolar {
            sheet,
            from: pref(r),
            length_mm: len_num(r),
            angle_deg: if r.chance(60) {
                (r.below(8) * 45) as f64
            } else {
                num(r)
            },
        },
        2 => Command::AddRectangle {
            sheet,
            corner: pref(r),
            width_mm: len_num(r),
            height_mm: len_num(r),
        },
        3 => Command::AddCurve {
            sheet,
            start: pref(r),
            c1: pt_mm(r),
            c2: pt_mm(r),
            end: pref(r),
        },
        4 => Command::AddPoint {
            sheet,
            at: pt_mm(r),
        },
        5 => Command::AddPointAt {
            sheet,
            point: pref(r),
        },
        6 => Command::AddSegmentPolar {
            sheet,
            from: PointRef::New(pt_mm(r)),
            length_mm: 20.0 + r.below(80) as f64,
            angle_deg: (r.below(12) * 30) as f64,
        },
        7 => Command::RenameProject { name: rname(r) },
        8 => Command::CreateSheet { name: rname(r) },
        9 => Command::RenameSheet {
            sheet,
            name: rname(r),
        },
        10 => Command::DeleteSheet { sheet },
        11 => Command::MoveSheet {
            sheet,
            new_index: r.below(5),
        },
        12 => Command::DuplicateSheet { sheet },
        13 => {
            let (sh, pcs) = if r.chance(35) {
                match s {
                    Some(s) => (s.clone(), p.pieces_of(s.id).cloned().collect::<Vec<_>>()),
                    None => ctx.donors[0].clone(),
                }
            } else {
                ctx.donors[r.below(ctx.donors.len())].clone()
            };
            Command::ImportSheet {
                sheet: Box::new(sh),
                pieces: pcs,
            }
        }
        14 => Command::AddPiece {
            sheet,
            name: rname(r),
            quantity: r.below(4) as u32,
            material: if r.chance(50) { Some(rname(r)) } else { None },
            mirrored_pair: r.chance(50),
        },
        15 => Command::SetPieceInfo {
            piece: pick(r, &pieces, PieceId(BOGUS)),
            name: if r.chance(80) {
                "Pièce".to_string()
            } else {
                rname(r)
            },
            quantity: 1 + r.below(3) as u32,
            material: None,
            mirrored_pair: r.chance(50),
        },
        16 => Command::DeletePiece {
            piece: pick(r, &pieces, PieceId(BOGUS)),
        },
        17 => {
            let outline: Vec<PieceEdge> = match s {
                Some(s) if r.chance(60) => contour::closed_contours(s)
                    .into_iter()
                    .nth(r.below(3))
                    .map(|c| {
                        c.edges
                            .iter()
                            .map(|e| PieceEdge {
                                entity: e.entity,
                                reversed: e.reversed,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => some(r, &ents, 4)
                    .into_iter()
                    .map(|entity| PieceEdge {
                        entity,
                        reversed: r.chance(50),
                    })
                    .collect(),
            };
            Command::SetPieceOutline {
                piece: pick(r, &pieces, PieceId(BOGUS)),
                outline,
                grainline: if r.chance(40) { Some(en(r)) } else { None },
            }
        }
        18 => Command::AddPerpendicular {
            sheet,
            reference: en(r),
            from: pref(r),
            length_mm: len_num(r),
            side: side(r),
        },
        19 => Command::AddParallel {
            sheet,
            reference: en(r),
            distance_mm: len_num(r),
            side: side(r),
        },
        20 | 21 => {
            let transform = match r.below(3) {
                0 => Transform::Translate {
                    dx_mm: num(r),
                    dy_mm: num(r),
                },
                1 => Transform::Rotate {
                    center: pt_mm(r),
                    angle_deg: if r.chance(70) {
                        (r.below(8) * 45) as f64
                    } else {
                        num(r)
                    },
                },
                _ => Transform::Mirror {
                    axis_point: pt_mm(r),
                    axis_angle_deg: if r.chance(70) {
                        (r.below(4) * 45) as f64
                    } else {
                        num(r)
                    },
                },
            };
            if k == 20 {
                Command::Transform {
                    sheet,
                    points: some(r, &pts, 5),
                    transform,
                }
            } else {
                Command::Duplicate {
                    sheet,
                    entities: some(r, &ents, 4),
                    transform,
                }
            }
        }
        22 => Command::AddPointOnSegment {
            sheet,
            segment: en(r),
            distance_mm: len_num(r),
            origin: if r.chance(50) {
                SegmentEnd::Start
            } else {
                SegmentEnd::End
            },
        },
        23 => Command::AddNotch {
            sheet,
            entity: en(r),
            distance_mm: len_num(r),
            side: side(r),
            length_mm: 2.0 + r.below(10) as f64,
            style: if r.chance(50) {
                NotchStyle::Single
            } else {
                NotchStyle::Double
            },
        },
        24 => Command::SetNotch {
            sheet,
            notch: pick(r, &notches, NotchId(BOGUS)),
            distance_mm: len_num(r),
            side: side(r),
            length_mm: 2.0 + r.below(10) as f64,
            style: if r.chance(50) {
                NotchStyle::Single
            } else {
                NotchStyle::Double
            },
        },
        25 => Command::DeleteNotch {
            sheet,
            notch: pick(r, &notches, NotchId(BOGUS)),
        },
        26 => Command::AddDimension {
            sheet,
            from: pt(r),
            to: pt(r),
            offset_mm: num(r),
        },
        27 => Command::SetDimensionOffset {
            sheet,
            dimension: pick(r, &dims, DimensionId(BOGUS)),
            offset_mm: num(r),
        },
        28 => Command::DeleteDimension {
            sheet,
            dimension: pick(r, &dims, DimensionId(BOGUS)),
        },
        29 => {
            if depth >= 2 {
                return Command::AddPoint {
                    sheet,
                    at: pt_mm(r),
                };
            }
            let n = 1 + r.below(3);
            Command::Batch {
                commands: (0..n).map(|_| gen(r, p, ctx, depth + 1)).collect(),
            }
        }
        30 => Command::MovePoint {
            sheet,
            point: pt(r),
            to: pt_mm(r),
        },
        31 => Command::SetPointDistance {
            sheet,
            point: derived(r),
            distance_mm: len_num(r),
        },
        32 => Command::SetPointParameter {
            sheet,
            point: derived(r),
            value: if r.chance(80) { r.unit() } else { num(r) },
        },
        33 => Command::MoveCurveHandle {
            sheet,
            curve: en(r),
            handle: if r.chance(50) {
                CurveHandle::Start
            } else {
                CurveHandle::End
            },
            to: pt_mm(r),
        },
        34 => Command::SetPointLabel {
            sheet,
            point: pt(r),
            label: rname(r),
        },
        35 => Command::SetPointRole {
            sheet,
            point: pt(r),
            role: if r.chance(50) {
                PointRole::Reference
            } else {
                PointRole::Construction
            },
        },
        36 => Command::SetPathRole {
            sheet,
            entity: en(r),
            role: PathRole::ALL[r.below(6)],
        },
        37 | 38 => Command::Delete {
            sheet,
            points: if k == 37 { some(r, &pts, 2) } else { vec![] },
            entities: some(r, &ents, 2),
        },
        39 => Command::AddLayer {
            sheet,
            name: rname(r),
        },
        40 => Command::RenameLayer {
            sheet,
            layer: pick(r, &layers, LayerId(BOGUS)),
            name: rname(r),
        },
        41 => Command::SetLayerVisible {
            sheet,
            layer: pick(r, &layers, LayerId(BOGUS)),
            visible: r.chance(50),
        },
        42 => Command::SetLayerLocked {
            sheet,
            layer: pick(r, &layers, LayerId(BOGUS)),
            locked: r.chance(50),
        },
        43 => Command::MoveLayer {
            sheet,
            layer: pick(r, &layers, LayerId(BOGUS)),
            new_index: r.below(4),
        },
        44 => Command::DeleteLayer {
            sheet,
            layer: pick(r, &layers, LayerId(BOGUS)),
            move_to: if r.chance(50) {
                Some(pick(r, &layers, LayerId(BOGUS)))
            } else {
                None
            },
        },
        45 => Command::SetObjectsLayer {
            sheet,
            points: some(r, &pts, 3),
            entities: some(r, &ents, 3),
            layer: if r.chance(80) {
                Some(pick(r, &layers, LayerId(BOGUS)))
            } else {
                None
            },
        },
        46 => Command::SetEntityName {
            sheet,
            entity: en(r),
            name: if r.chance(80) { Some(rname(r)) } else { None },
        },
        47 => Command::CreateGroup {
            sheet,
            entities: some(r, &ents, 4),
        },
        48 => Command::Ungroup {
            sheet,
            group: pick(r, &groups, GroupId(BOGUS)),
        },
        57 => Command::AddCircle {
            sheet,
            center: pref(r),
            radius_mm: len_num(r),
        },
        58 => Command::AddCircleThreePoints {
            sheet,
            a: pref(r),
            b: pref(r),
            c: pref(r),
        },
        59 => Command::AddCircleTwoPointsRadius {
            sheet,
            a: pref(r),
            b: pref(r),
            radius_mm: len_num(r),
            side: side(r),
        },
        60 => Command::AddArc {
            sheet,
            center: pref(r),
            radius_mm: len_num(r),
            start_deg: if r.chance(70) {
                (r.below(12) * 30) as f64
            } else {
                num(r)
            },
            end_deg: if r.chance(70) {
                (r.below(12) * 30) as f64
            } else {
                num(r)
            },
            ccw: r.chance(60),
        },
        61 => Command::AddArcThreePoints {
            sheet,
            start: pref(r),
            through: pref(r),
            end: pref(r),
        },
        62 => Command::AddArcTwoPointsRadius {
            sheet,
            start: pref(r),
            end: pref(r),
            radius_mm: len_num(r),
            side: side(r),
            ccw: r.chance(50),
        },
        63 => Command::SetCircleRadius {
            sheet,
            entity: en(r),
            radius_mm: len_num(r),
        },
        64 => Command::AddSpline {
            sheet,
            points: (0..r.below(6)).map(|_| pref(r)).collect(),
        },
        65 => Command::AddSplinePoint {
            sheet,
            spline: if r.chance(90) {
                pick(r, &spline_ents, EntityId(BOGUS))
            } else {
                en(r)
            },
            index: r.below(8),
            point: pref(r),
        },
        66 => Command::RemoveSplinePoint {
            sheet,
            spline: if r.chance(90) {
                pick(r, &spline_ents, EntityId(BOGUS))
            } else {
                en(r)
            },
            index: r.below(8),
        },
        67 => {
            let tangent = |r: &mut Rng| {
                if r.chance(50) {
                    Some(OffsetMm {
                        dx: len_num(r),
                        dy: num(r),
                    })
                } else {
                    None
                }
            };
            Command::SetSplineNode {
                sheet,
                spline: if r.chance(90) {
                    pick(r, &spline_ents, EntityId(BOGUS))
                } else {
                    en(r)
                },
                index: r.below(6),
                corner: r.chance(40),
                tangent: tangent(r),
                tangent_in: tangent(r),
            }
        }
        68 => Command::CurveToSpline {
            sheet,
            curve: if r.chance(90) {
                pick(r, &curve_ents, EntityId(BOGUS))
            } else {
                en(r)
            },
        },
        69 => Command::SplineToCurve {
            sheet,
            spline: if r.chance(90) {
                pick(r, &spline_ents, EntityId(BOGUS))
            } else {
                en(r)
            },
        },
        70 => {
            let first = pick(r, &round_ents, EntityId(BOGUS));
            let second = if r.chance(97) {
                pick(r, &round_ents, EntityId(BOGUS))
            } else {
                first
            };
            Command::FilletPaths {
                sheet,
                first,
                second,
                radius_mm: blend_len(r),
                pick_first: aim_on(r, first),
                pick_second: aim_on(r, second),
            }
        }
        71 => {
            let first = pick(r, &seg_ents, EntityId(BOGUS));
            let second = if r.chance(95) {
                pick(r, &seg_ents, EntityId(BOGUS))
            } else {
                en(r)
            };
            Command::ChamferPaths {
                sheet,
                first,
                second,
                distance_first_mm: blend_len(r),
                distance_second_mm: blend_len(r),
                pick_first: aim_on(r, first),
                pick_second: aim_on(r, second),
            }
        }
        72 => {
            let target = pick(r, &round_ents, EntityId(BOGUS));
            Command::AddTangentLine {
                sheet,
                from: pref(r),
                target,
                pick: aim_on(r, target),
                length_mm: if r.chance(50) {
                    Some(blend_len(r))
                } else {
                    None
                },
            }
        }
        73 => {
            let entity = if r.chance(92) {
                pick(r, &round_ents, EntityId(BOGUS))
            } else {
                en(r)
            };
            Command::AddTangentAt {
                sheet,
                entity,
                at: if r.chance(50) {
                    TangentAt::Fraction(if r.chance(90) { r.unit() } else { num(r) })
                } else {
                    TangentAt::Near(aim_on(r, entity))
                },
                length_mm: blend_len(r),
                backwards: r.chance(50),
            }
        }
        79 => {
            let entity = if r.chance(85) {
                pick(r, &round_ents, EntityId(BOGUS))
            } else {
                en(r)
            };
            Command::ExtendPath {
                sheet,
                entity,
                end: if r.chance(50) {
                    SegmentEnd::Start
                } else {
                    SegmentEnd::End
                },
                delta_mm: match r.below(10) {
                    0..=5 => r.below(70) as f64 - 25.0,
                    6 => r.unit() * 30.0 - 10.0,
                    7 => 0.0,
                    _ => num(r),
                },
            }
        }
        80 => {
            let entity = if r.chance(85) {
                pick(r, &round_ents, EntityId(BOGUS))
            } else {
                en(r)
            };
            let target = if r.chance(5) { entity } else { en(r) };
            Command::TrimPathTo {
                sheet,
                entity,
                end: if r.chance(50) {
                    SegmentEnd::Start
                } else {
                    SegmentEnd::End
                },
                target,
                pick: aim_on(r, target),
            }
        }
        81 => {
            let entity = en(r);
            // Le plus souvent un point exactement sur le tracé ; parfois n'importe où.
            let on_path = s.and_then(|s| {
                let len = s.path_length_mm(entity).ok()?;
                Some(s.path_point_at(entity, len * r.unit()).ok()?.0)
            });
            Command::SplitPath {
                sheet,
                entity,
                at: match on_path {
                    Some(q) if r.chance(88) => q,
                    _ => pt_mm(r),
                },
            }
        }
        82 => {
            // Deux tracés adjacents de même type (sinon n'importe lesquels).
            let first = en(r);
            let adjacent: Vec<EntityId> = s
                .map(|s| {
                    let ends = |e: EntityId| s.entity_points(e).ok();
                    let kind =
                        |e: EntityId| s.entity(e).ok().map(|x| std::mem::discriminant(&x.kind));
                    match ends(first) {
                        Some((a, b)) => s
                            .entities
                            .iter()
                            .filter(|x| x.id != first && kind(x.id) == kind(first))
                            .filter(|x| {
                                ends(x.id)
                                    .is_some_and(|(c, d)| c == a || c == b || d == a || d == b)
                            })
                            .map(|x| x.id)
                            .collect(),
                        None => Vec::new(),
                    }
                })
                .unwrap_or_default();
            let second = if r.chance(85) && !adjacent.is_empty() {
                pick(r, &adjacent, EntityId(BOGUS))
            } else {
                en(r)
            };
            Command::MergePaths {
                sheet,
                first,
                second,
            }
        }
        83 => Command::ReversePath {
            sheet,
            entity: en(r),
        },
        88 => {
            // Plutôt une chaîne détectée depuis un tracé ; parfois n'importe quoi.
            let paths: Vec<EntityId> = match r.below(10) {
                0..=5 => {
                    let start = en(r);
                    s.and_then(|sh| pf_core::offset::detect_chain(sh, start).ok())
                        .unwrap_or_else(|| vec![start])
                }
                6..=7 => some(r, &ents, 4),
                8 => Vec::new(),
                _ => vec![en(r), en(r)],
            };
            let magnitude = match r.below(10) {
                0..=6 => 0.5 + r.below(30) as f64,
                7 => r.unit() * 100.0,
                _ => num(r),
            };
            Command::OffsetPaths {
                sheet,
                paths,
                distance_mm: if r.chance(50) { magnitude } else { -magnitude },
                corner: if r.chance(50) {
                    pf_geometry::CornerStyle::Miter
                } else {
                    pf_geometry::CornerStyle::Round
                },
                role: pick(r, &PathRole::ALL, PathRole::Construction),
            }
        }
        89 | 90 => {
            // Plutôt un axe marqué comme tel / un axe déjà utilisé ; parfois n'importe quoi.
            let axis_guides: Vec<GuideId> = s
                .map(|s| s.guides.iter().filter(|g| g.axis).map(|g| g.id).collect())
                .unwrap_or_default();
            let used: Vec<MirrorAxis> = s
                .map(|s| {
                    s.points
                        .iter()
                        .filter_map(|q| match q.definition {
                            PointDefinition::Mirror { axis, .. } => Some(axis),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let axis = match r.below(10) {
                0..=5 if !axis_guides.is_empty() => MirrorAxis::Guide {
                    guide: pick(r, &axis_guides, GuideId(BOGUS)),
                },
                6..=7 if k == 90 && !used.is_empty() => pick(
                    r,
                    &used,
                    MirrorAxis::Segment {
                        segment: EntityId(BOGUS),
                    },
                ),
                8 => MirrorAxis::Guide {
                    guide: pick(r, &guide_ids, GuideId(BOGUS)),
                },
                6..=7 => MirrorAxis::Segment { segment: en(r) },
                _ => MirrorAxis::Segment {
                    segment: EntityId(BOGUS),
                },
            };
            // Sources plutôt hors des miroirs existants (sinon refus certain).
            let plain_pts: Vec<PointId> = s
                .map(|s| {
                    s.points
                        .iter()
                        .filter(|q| !q.definition.is_mirror())
                        .map(|q| q.id)
                        .collect()
                })
                .unwrap_or_default();
            let plain_ents: Vec<EntityId> = s
                .map(|s| {
                    s.entities
                        .iter()
                        .filter(|e| {
                            e.kind
                                .all_points()
                                .iter()
                                .all(|p| s.point(*p).is_ok_and(|q| !q.definition.is_mirror()))
                        })
                        .map(|e| e.id)
                        .collect()
                })
                .unwrap_or_default();
            if k == 89 && axis_guides.is_empty() && guide_ids.is_empty() && r.chance(70) {
                Command::AddGuide {
                    sheet,
                    x_mm: r.below(60) as f64,
                    y_mm: r.below(60) as f64,
                    angle_deg: (r.below(4) * 45) as f64,
                    axis: true,
                    layer: None,
                }
            } else if k == 89 && axis_guides.is_empty() && !guide_ids.is_empty() && r.chance(60) {
                // Amorce : marque un guide comme axe pour que la symétrie ait une chance.
                Command::SetGuideAxis {
                    sheet,
                    guide: pick(r, &guide_ids, GuideId(BOGUS)),
                    axis: true,
                }
            } else if k == 89 {
                let (sp, se) = if r.chance(85) {
                    (some(r, &plain_pts, 2), some(r, &plain_ents, 3))
                } else {
                    (some(r, &pts, 3), some(r, &ents, 3))
                };
                Command::LinkMirror {
                    sheet,
                    points: sp,
                    entities: se,
                    axis,
                }
            } else {
                Command::UnfoldMirror { sheet, axis }
            }
        }
        84 => Command::AddGuide {
            sheet,
            x_mm: num(r),
            y_mm: num(r),
            angle_deg: if r.chance(60) {
                (r.below(8) * 45) as f64
            } else {
                num(r)
            },
            axis: r.chance(30),
            layer: if r.chance(30) {
                Some(pick(r, &layers, LayerId(BOGUS)))
            } else {
                None
            },
        },
        85 => Command::MoveGuide {
            sheet,
            guide: pick(r, &guide_ids, GuideId(BOGUS)),
            x_mm: num(r),
            y_mm: num(r),
            angle_deg: num(r),
        },
        86 => Command::DeleteGuide {
            sheet,
            guide: pick(r, &guide_ids, GuideId(BOGUS)),
        },
        87 => Command::SetGuideAxis {
            sheet,
            guide: pick(r, &guide_ids, GuideId(BOGUS)),
            axis: r.chance(50),
        },
        74 | 75 => {
            // Plutôt une courbe contre un tracé quelconque ; parfois n'importe quoi
            // (même tracé deux fois, tracé absent, point visé extrême).
            let curves: Vec<EntityId> = spline_ents
                .iter()
                .chain(curve_ents.iter())
                .copied()
                .collect();
            let first = if k == 74 && !curves.is_empty() && r.chance(75) {
                pick(r, &curves, EntityId(BOGUS))
            } else {
                en(r)
            };
            let second = if k == 75 && r.chance(10) {
                first
            } else {
                en(r)
            };
            let near = if k == 75 && r.chance(40) {
                PointMm::new(num(r), num(r))
            } else {
                aim_on(r, first)
            };
            Command::AddPointAt {
                sheet,
                point: PointRef::Derived(PointDefinition::TraceIntersection {
                    first,
                    second,
                    near: near.into(),
                }),
            }
        }
        76 => {
            let small = |r: &mut Rng| -> f64 {
                match r.below(10) {
                    0..=6 => r.below(60) as f64 - 10.0,
                    7 => r.unit() * 30.0,
                    _ => num(r),
                }
            };
            let near = if r.chance(70) {
                s.and_then(|s| s.position(pt(r)).ok())
                    .unwrap_or_else(|| pt_mm(r))
            } else {
                pt_mm(r)
            };
            let origin = if r.chance(50) {
                SegmentEnd::Start
            } else {
                SegmentEnd::End
            };
            let d = match r.below(7) {
                0 => PointDefinition::AlongCurve {
                    curve: en(r),
                    distance_mm: if r.chance(80) {
                        r.unit() * 150.0
                    } else {
                        num(r)
                    },
                    origin,
                },
                1 => PointDefinition::Offset {
                    from: pt(r),
                    dx_mm: small(r),
                    dy_mm: small(r),
                },
                2 => PointDefinition::Midpoint {
                    first: pt(r),
                    second: pt(r),
                },
                3 => PointDefinition::Projection {
                    point: pt(r),
                    trace: en(r),
                },
                4 => PointDefinition::Bisector {
                    vertex: pt(r),
                    first: pt(r),
                    second: pt(r),
                    distance_mm: if r.chance(80) {
                        1.0 + r.below(40) as f64
                    } else {
                        num(r)
                    },
                },
                5 => PointDefinition::Polar {
                    from: pt(r),
                    distance_mm: if r.chance(80) {
                        1.0 + r.below(60) as f64
                    } else {
                        num(r)
                    },
                    angle_deg: small(r),
                    reference: if r.chance(50) { Some(en(r)) } else { None },
                },
                _ => PointDefinition::Compass {
                    center_a: pt(r),
                    center_b: pt(r),
                    radius_a_mm: if r.chance(80) {
                        1.0 + r.below(80) as f64
                    } else {
                        num(r)
                    },
                    radius_b_mm: if r.chance(80) {
                        1.0 + r.below(80) as f64
                    } else {
                        num(r)
                    },
                    near: near.into(),
                },
            };
            Command::AddPointAt {
                sheet,
                point: PointRef::Derived(d),
            }
        }
        77 => {
            let n = match r.below(6) {
                0 => 0,
                1 => 1,
                2 => 3,
                _ => 2,
            };
            Command::SetPointValues {
                sheet,
                point: derived(r),
                values: (0..n)
                    .map(|_| match r.below(8) {
                        0..=5 => r.below(120) as f64 - 10.0,
                        6 => r.unit() * 90.0,
                        _ => num(r),
                    })
                    .collect(),
            }
        }
        78 => {
            let spec = if r.chance(50) {
                DivisionSpec::Equal {
                    parts: match r.below(8) {
                        0 => 0,
                        1 => 1,
                        2 if r.chance(5) => 1000,
                        3 => 1001,
                        4 => usize::MAX,
                        _ => 2 + r.below(9),
                    },
                }
            } else {
                let n = match r.below(6) {
                    0 => 0,
                    1 => 1,
                    _ => 2 + r.below(5),
                };
                DivisionSpec::Proportions {
                    values: (0..n)
                        .map(|_| match r.below(8) {
                            0..=4 => 1.0 + r.below(5) as f64,
                            5 => r.unit(),
                            _ => num(r),
                        })
                        .collect(),
                }
            };
            Command::DivideTrace {
                sheet,
                trace: en(r),
                spec,
                start: if r.chance(30) { Some(pt(r)) } else { None },
            }
        }
        49 | 50 => {
            if ctx.bank.is_empty() {
                return Command::AddPoint {
                    sheet,
                    at: pt_mm(r),
                };
            }
            Command::PasteFragment {
                sheet,
                fragment: Box::new(ctx.bank[r.below(ctx.bank.len())].clone()),
                layer: if r.chance(40) {
                    Some(pick(r, &layers, LayerId(BOGUS)))
                } else {
                    None
                },
            }
        }
        _ => {
            if depth >= 2 {
                return Command::AddPoint {
                    sheet,
                    at: pt_mm(r),
                };
            }
            Command::InLayer {
                sheet,
                layer: pick(r, &layers, LayerId(BOGUS)),
                command: Box::new(gen(r, p, ctx, depth + 1)),
            }
        }
    }
}

pub fn variant_name(c: &Command) -> String {
    let d = format!("{c:?}");
    d.split([' ', '(', '{']).next().unwrap().to_string()
}

pub fn nested_names(c: &Command, out: &mut Vec<String>) {
    out.push(variant_name(c));
    match c {
        Command::Batch { commands } => commands.iter().for_each(|c| nested_names(c, out)),
        Command::InLayer { command, .. } => nested_names(command, out),
        _ => {}
    }
}

#[derive(Default)]
pub struct Stats {
    pub attempted: BTreeMap<String, usize>,
    pub accepted: BTreeMap<String, usize>,
    pub commands: usize,
    pub sequences: usize,
}

pub fn tokens_with_nan(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+'))
        .find(|t| {
            matches!(
                *t,
                "NaN" | "nan" | "inf" | "-inf" | "+inf" | "Infinity" | "-Infinity"
            )
        })
        .map(str::to_string)
}

/// Un nombre trop grand pour tenir en `f64` fini ressortirait avec ~300 chiffres.
pub fn check_export(p: &Project, label: &str) {
    use pf_export::{pdf::export_pdf, svg::export_svg, ExportError, Scene};
    let mut scenes = Vec::new();
    for s in &p.sheets {
        let scene = catch_unwind(AssertUnwindSafe(|| Scene::from_sheet(p, s.id)))
            .unwrap_or_else(|_| panic!("{label} : PANIC dans Scene::from_sheet"));
        let scene = match scene {
            Ok(sc) => sc,
            Err(ExportError::EmptySheet(_)) => continue,
            Err(e) => panic!("{label} : scène refusée pour un projet valide : {e}"),
        };
        let svg = catch_unwind(AssertUnwindSafe(|| export_svg(&scene)))
            .unwrap_or_else(|_| panic!("{label} : PANIC dans export_svg"));
        match svg {
            Ok(text) => {
                if let Some(t) = tokens_with_nan(text.as_bytes()) {
                    panic!("{label} : jeton non fini « {t} » dans le SVG");
                }
            }
            Err(ExportError::EmptySheet(_)) => {}
            Err(e) => panic!("{label} : SVG refusé : {e}"),
        }
        scenes.push(scene);
    }
    let pdf = catch_unwind(AssertUnwindSafe(|| export_pdf(&scenes)))
        .unwrap_or_else(|_| panic!("{label} : PANIC dans export_pdf"));
    match pdf {
        Ok(out) => {
            if let Some(t) = tokens_with_nan(&out.bytes) {
                panic!("{label} : jeton non fini « {t} » dans le PDF");
            }
        }
        Err(ExportError::TooManyPages { .. }) | Err(ExportError::EmptySheet(_)) => {}
        Err(e) => panic!("{label} : PDF refusé : {e}"),
    }
}

pub fn run_sequence(seed: u64, len: usize, ctx: &mut Ctx, stats: &mut Stats, with_export: bool) {
    let mut r = Rng(seed);
    let mut doc = Document::new(Project::new("Aléa"));
    ctx.bank.clear();
    // états successifs (JSON brut) pour la remontée complète de l'historique
    let mut states: Vec<String> = vec![snap(doc.project())];
    for step in 0..len {
        let label = format!("graine {seed} pas {step}");
        let cmd = gen(&mut r, doc.project(), ctx, 0);
        let mut names = Vec::new();
        nested_names(&cmd, &mut names);
        for n in &names {
            *stats.attempted.entry(n.clone()).or_default() += 1;
        }
        let top = variant_name(&cmd);
        let is_trace = matches!(
            &cmd,
            Command::AddPointAt {
                point: PointRef::Derived(PointDefinition::TraceIntersection { .. }),
                ..
            }
        );
        let is_relative = matches!(
            &cmd,
            Command::AddPointAt {
                point: PointRef::Derived(
                    PointDefinition::AlongCurve { .. }
                        | PointDefinition::Offset { .. }
                        | PointDefinition::Midpoint { .. }
                        | PointDefinition::Projection { .. }
                        | PointDefinition::Bisector { .. }
                        | PointDefinition::Polar { .. }
                        | PointDefinition::Compass { .. }
                ),
                ..
            }
        );
        if is_relative {
            *stats.attempted.entry("PointRelatif".into()).or_default() += 1;
        }
        if is_trace {
            *stats
                .attempted
                .entry("TraceIntersection".into())
                .or_default() += 1;
        }
        let before_project = doc.project().clone();
        let before = snap(&before_project);
        let result = catch_unwind(AssertUnwindSafe(|| doc.execute(cmd.clone())))
            .unwrap_or_else(|_| panic!("{label} : PANIC sur {cmd:?}"));
        stats.commands += 1;
        match result {
            Err(e) => {
                assert!(!e.to_string().trim().is_empty(), "{label} : message vide");
                assert_eq!(
                    snap(doc.project()),
                    before,
                    "{label} : refus mais modifié ({cmd:?})"
                );
                assert!(doc.project().validate().is_ok());
            }
            Ok(_) => {
                if is_relative {
                    *stats.accepted.entry("PointRelatif".into()).or_default() += 1;
                }
                if is_trace {
                    *stats
                        .accepted
                        .entry("TraceIntersection".into())
                        .or_default() += 1;
                }
                *stats.accepted.entry(top).or_default() += 1;
                for n in names.iter().skip(1) {
                    *stats
                        .accepted
                        .entry(format!("{n} (imbriquée)"))
                        .or_default() += 1;
                }
                doc.project()
                    .validate()
                    .unwrap_or_else(|e| panic!("{label} : accepté mais invalide : {e} ({cmd:?})"));
                let after = snap(doc.project());
                if *doc.project() != before_project {
                    assert!(
                        doc.can_undo(),
                        "{label} : rien à annuler après un changement"
                    );
                    assert!(doc.undo());
                    assert_eq!(
                        snap(doc.project()),
                        before,
                        "{label} : annuler inexact ({cmd:?})"
                    );
                    assert!(doc.redo());
                    assert_eq!(
                        snap(doc.project()),
                        after,
                        "{label} : rétablir inexact ({cmd:?})"
                    );
                    states.push(after.clone());
                }
                // relecture idempotente
                let json = persist::to_json(doc.project())
                    .unwrap_or_else(|e| panic!("{label} : non sérialisable : {e}"));
                let back = persist::from_json(&json)
                    .unwrap_or_else(|e| panic!("{label} : non relisible : {e} ({cmd:?})"));
                assert_eq!(
                    &back,
                    doc.project(),
                    "{label} : relecture différente ({cmd:?})"
                );
                assert_eq!(
                    persist::to_json(&back).unwrap(),
                    json,
                    "{label} : sérialisation non idempotente"
                );
            }
        }
        // alimente le presse-papiers de test
        if r.chance(12) {
            let p = doc.project();
            let sh = p.sheets[r.below(p.sheets.len())].id;
            let (pts, ents, notches, dims, _, _) = ids(p.sheet(sh).ok());
            let gids: Vec<GuideId> = p
                .sheet(sh)
                .map(|s| s.guides.iter().map(|g| g.id).collect())
                .unwrap_or_default();
            let (sp, se, sn, sd) = (
                some(&mut r, &pts, 2),
                some(&mut r, &ents, 4),
                some(&mut r, &notches, 1),
                some(&mut r, &dims, 1),
            );
            let sg = if ctx.guides {
                some(&mut r, &gids, 2)
            } else {
                Vec::new()
            };
            if let Ok(f) = p.copy_fragment_with_guides(sh, &sp, &se, &sn, &sd, &sg) {
                // un fragment produit par le logiciel doit toujours se relire
                Fragment::from_json(&f.to_json())
                    .unwrap_or_else(|e| panic!("{label} : fragment non relisible : {e}"));
                ctx.bank.push(f);
            }
        }
        if with_export && step % 25 == 24 {
            check_export(doc.project(), &label);
        }
    }
    // remontée complète de l'historique puis rejeu complet
    let final_state = snap(doc.project());
    let mut i = states.len() - 1;
    assert_eq!(states[i], final_state);
    while i > 0 {
        assert!(
            doc.undo(),
            "graine {seed} : annulation impossible à l'étape {i}"
        );
        i -= 1;
        assert_eq!(
            snap(doc.project()),
            states[i],
            "graine {seed} : remontée inexacte à {i}"
        );
    }
    assert!(!doc.undo());
    while i + 1 < states.len() {
        assert!(doc.redo(), "graine {seed} : rétablissement impossible");
        i += 1;
        assert_eq!(
            snap(doc.project()),
            states[i],
            "graine {seed} : rejeu inexact à {i}"
        );
    }
    assert!(!doc.redo());
    if with_export {
        check_export(doc.project(), &format!("graine {seed} final"));
    }
    stats.sequences += 1;
}

pub fn donors(arcs: bool, splines: bool) -> Vec<(Sheet, Vec<Piece>)> {
    let mut v = Vec::new();
    for p in [
        pf_core::demo_project().unwrap(),
        pf_core::demo_piece_project().unwrap(),
        if splines {
            rich_project_with_splines()
        } else if arcs {
            rich_project_with_arcs()
        } else {
            rich_project()
        },
    ] {
        for s in &p.sheets {
            v.push((s.clone(), p.pieces_of(s.id).cloned().collect()));
        }
    }
    v
}

pub fn campagne(first_seed: u64, sequences: u64) -> Stats {
    campagne_avec(first_seed, sequences, false)
}

pub fn campagne_avec(first_seed: u64, sequences: u64, arcs: bool) -> Stats {
    campagne_mode(first_seed, sequences, arcs, false)
}

pub fn campagne_mode(first_seed: u64, sequences: u64, arcs: bool, splines: bool) -> Stats {
    campagne_complete(first_seed, sequences, arcs, splines, false, false)
}

pub fn campagne_complete(
    first_seed: u64,
    sequences: u64,
    arcs: bool,
    splines: bool,
    tangency: bool,
    traces: bool,
) -> Stats {
    campagne_relative(
        first_seed, sequences, arcs, splines, tangency, traces, false,
    )
}

pub fn campagne_relative(
    first_seed: u64,
    sequences: u64,
    arcs: bool,
    splines: bool,
    tangency: bool,
    traces: bool,
    relative: bool,
) -> Stats {
    campagne_ctx(
        first_seed,
        sequences,
        Ctx {
            donors: donors(arcs, splines),
            bank: Vec::new(),
            arcs,
            splines,
            tangency,
            traces,
            relative,
            divisions: false,
            paths: false,
            guides: false,
            offsets: false,
            mirrors: false,
        },
    )
}

/// US-02.7 : tous les générateurs précédents + modifier un tracé.
pub fn campagne_paths(first_seed: u64, sequences: u64) -> Stats {
    campagne_ctx(
        first_seed,
        sequences,
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
            guides: false,
            offsets: false,
            mirrors: false,
        },
    )
}

/// US-02.8 : tous les générateurs précédents + les lignes d'aide.
pub fn campagne_guides(first_seed: u64, sequences: u64) -> Stats {
    campagne_ctx(
        first_seed,
        sequences,
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
            offsets: false,
            mirrors: false,
        },
    )
}

/// US-02.10 : tous les générateurs précédents + les décalages de contour.
pub fn campagne_offsets(first_seed: u64, sequences: u64) -> Stats {
    campagne_ctx(
        first_seed,
        sequences,
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
            mirrors: false,
        },
    )
}

/// US-02.11 : tous les générateurs précédents + symétries liées et dépliage.
pub fn campagne_mirrors(first_seed: u64, sequences: u64) -> Stats {
    campagne_ctx(
        first_seed,
        sequences,
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
        },
    )
}

/// US-02.6 : tous les générateurs précédents + les divisions.
pub fn campagne_divisions(first_seed: u64, sequences: u64) -> Stats {
    campagne_ctx(
        first_seed,
        sequences,
        Ctx {
            donors: donors(true, true),
            bank: Vec::new(),
            arcs: true,
            splines: true,
            tangency: false,
            traces: true,
            relative: true,
            divisions: true,
            paths: false,
            guides: false,
            offsets: false,
            mirrors: false,
        },
    )
}

pub fn campagne_ctx(first_seed: u64, sequences: u64, mut ctx: Ctx) -> Stats {
    let mut stats = Stats::default();
    for i in 0..sequences {
        let seed = first_seed + i;
        // l'export (coûteux) n'est vérifié que sur une séquence sur trois
        run_sequence(
            seed.wrapping_mul(0x2545_F491_4F6C_DD1D),
            50,
            &mut ctx,
            &mut stats,
            i % 3 == 0,
        );
    }
    stats
}

pub fn report(name: &str, stats: &Stats) {
    eprintln!(
        "{name} : {} séquences, {} commandes",
        stats.sequences, stats.commands
    );
    for (k, n) in &stats.attempted {
        eprintln!(
            "  {k:<22} tentées {n:>5}  acceptées {:>5}",
            stats.accepted.get(k).copied().unwrap_or(0)
        );
    }
}

pub const ALL_VARIANTS: [&str; 52] = [
    "RenameProject",
    "CreateSheet",
    "RenameSheet",
    "DeleteSheet",
    "MoveSheet",
    "DuplicateSheet",
    "ImportSheet",
    "AddPiece",
    "SetPieceInfo",
    "DeletePiece",
    "SetPieceOutline",
    "AddPoint",
    "AddPointAt",
    "AddSegment",
    "AddSegmentPolar",
    "AddPerpendicular",
    "AddRectangle",
    "AddParallel",
    "Transform",
    "Duplicate",
    "AddPointOnSegment",
    "AddNotch",
    "SetNotch",
    "DeleteNotch",
    "AddDimension",
    "SetDimensionOffset",
    "DeleteDimension",
    "Batch",
    "AddCurve",
    "MovePoint",
    "SetPointDistance",
    "SetPointParameter",
    "MoveCurveHandle",
    "SetPointLabel",
    "SetPointRole",
    "SetPathRole",
    "Delete",
    "AddLayer",
    "RenameLayer",
    "SetLayerVisible",
    "SetLayerLocked",
    "MoveLayer",
    "DeleteLayer",
    "SetObjectsLayer",
    "SetEntityName",
    "CreateGroup",
    "Ungroup",
    "PasteFragment",
    "InLayer",
    // sentinelles : doivent rester vraies si un variant est ajouté sans mise à jour
    "AddPoint",
    "AddPoint",
    "AddPoint",
];

pub fn assert_coverage(stats: &Stats) {
    for v in ALL_VARIANTS {
        assert!(stats.attempted.contains_key(v), "variant {v} jamais tenté");
    }
    // commandes qui doivent être acceptées au moins une fois par campagne
    for v in [
        "AddSegmentPolar",
        "AddRectangle",
        "AddCurve",
        "Transform",
        "Duplicate",
        "AddNotch",
        "AddDimension",
        "CreateGroup",
        "AddLayer",
        "DuplicateSheet",
        "ImportSheet",
        "PasteFragment",
        "AddPiece",
        "Delete",
        "Batch",
        "InLayer",
        "MovePoint",
        "SetPointDistance",
        "SetPointParameter",
        "SetNotch",
        "MoveCurveHandle",
        "Ungroup",
        "DeleteLayer",
        "SetPieceInfo",
    ] {
        assert!(
            stats.accepted.get(v).copied().unwrap_or(0) > 0,
            "variant {v} jamais accepté : couverture insuffisante"
        );
    }
}

/// Projet riche + deux lignes d'aide (dont un axe, une dans un calque).
pub fn rich_project_with_guides() -> Project {
    let mut p = rich_project();
    let sheet = p.sheets[0].id;
    let layer = p.sheets[0].layers[0].id;
    p.apply(Command::AddGuide {
        sheet,
        x_mm: 0.0,
        y_mm: 60.0,
        angle_deg: 0.0,
        axis: false,
        layer: None,
    })
    .unwrap();
    p.apply(Command::AddGuide {
        sheet,
        x_mm: 85.0,
        y_mm: 0.0,
        angle_deg: 60.0,
        axis: true,
        layer: Some(layer),
    })
    .unwrap();
    p
}

// ---------------------------------------------------------------------------
// US-04.1 — extraction de pièces : tracés aléatoires, clics aléatoires
// ---------------------------------------------------------------------------

/// Polyligne d'un contour de pièce, d'après les tracés du projet (échantillonnage fin).
pub fn piece_polygon(p: &Project, piece: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(piece).unwrap();
    let mut poly: Vec<PointMm> = Vec::new();
    for e in &p.piece(piece).unwrap().outline {
        let mut pts = sheet.shape(e.entity).unwrap().sample(400);
        if e.reversed {
            pts.reverse();
        }
        poly.extend(pts);
    }
    poly
}

pub fn dist_to_polyline(poly: &[PointMm], q: PointMm) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            pf_geometry::project_on_segment(q, poly[i], poly[(i + 1) % n])
                .map(|r| r.1)
                .unwrap_or(f64::INFINITY)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Dessine 3 à 7 formes au hasard (rectangles, cercles, arcs, courbes, segments libres).
pub fn random_drawing(r: &mut Rng, p: &mut Project, sheet: SheetId) {
    let at = |r: &mut Rng| PointMm::new(r.unit() * 200.0, r.unit() * 200.0);
    for _ in 0..(3 + r.below(5)) {
        let cmd = match r.below(6) {
            0 | 1 => Command::AddRectangle {
                sheet,
                corner: PointRef::New(at(r)),
                width_mm: 20.0 + r.unit() * 120.0,
                height_mm: 20.0 + r.unit() * 120.0,
            },
            2 => Command::AddCircle {
                sheet,
                center: PointRef::New(at(r)),
                radius_mm: 10.0 + r.unit() * 60.0,
            },
            3 => Command::AddArc {
                sheet,
                center: PointRef::New(at(r)),
                radius_mm: 15.0 + r.unit() * 60.0,
                start_deg: r.unit() * 360.0,
                end_deg: r.unit() * 360.0,
                ccw: r.chance(70),
            },
            4 => Command::AddCurve {
                sheet,
                start: PointRef::New(at(r)),
                c1: at(r),
                c2: at(r),
                end: PointRef::New(at(r)),
            },
            _ => Command::AddSegment {
                sheet,
                from: PointRef::New(at(r)),
                to: PointRef::New(at(r)),
            },
        };
        let _ = p.apply(cmd);
    }
}

// ---------------------------------------------------------------------------
// US-04.3 — éléments internes de pièce : campagne aléatoire à graine fixe
// ---------------------------------------------------------------------------

/// Chaque élément interne désigne un objet vivant de la feuille de sa pièce
/// (toute feuille pour une pièce détachée) ; aucun doublon ; jamais un tracé du
/// contour ni le droit-fil ; type cohérent avec la cible.
pub fn assert_no_dead_internal(p: &Project, label: &str) {
    for piece in &p.pieces {
        let sheets: Vec<&Sheet> = match piece.link {
            PieceLink::Linked { sheet } => vec![p.sheet(sheet).unwrap()],
            PieceLink::Detached => p.sheets.iter().collect(),
        };
        let mut seen = Vec::new();
        for el in &piece.internal {
            let alive = sheets.iter().any(|s| match el.target {
                InternalTarget::Path { entity } => s.entity(entity).is_ok(),
                InternalTarget::Point { point } => s.point(point).is_ok(),
                InternalTarget::Notch { notch } => s.notch(notch).is_ok(),
            });
            assert!(alive, "référence morte {el:?} : {label}");
            assert!(el.kind.fits(el.target), "type incohérent {el:?} : {label}");
            assert!(!seen.contains(&el.target), "doublon {el:?} : {label}");
            seen.push(el.target);
            if let InternalTarget::Path { entity } = el.target {
                assert!(
                    !piece.outline.iter().any(|e| e.entity == entity)
                        && piece.grainline != Some(entity),
                    "tracé interne dans le contour : {label}"
                );
            }
        }
    }
}

pub fn wild_coord(r: &mut Rng) -> f64 {
    match r.below(14) {
        0 => 1e300,
        1 => f64::NAN,
        2 => -0.0,
        3 => 1e7,
        4 => f64::INFINITY,
        5 => -1e6,
        6 => 1e6,
        7 => -1e300,
        8 => 0.0,
        _ => r.unit() * 1200.0 - 600.0,
    }
}

pub fn wild_rotation(r: &mut Rng) -> f64 {
    match r.below(16) {
        0 => 1e300,
        1 => f64::NAN,
        2 => -0.0,
        3 => -1e-300,
        4 => 360.0,
        5 => 1e6,
        6 => -1e6,
        7 => 1e6 + 1.0,
        8 => f64::INFINITY,
        9 => -720.0,
        10 => 90.0,
        _ => r.unit() * 720.0 - 360.0,
    }
}

/// Droite de coupe : le plus souvent à travers la boîte d'une pièce, parfois extrême.
pub fn sj_line(r: &mut Rng, cur: &Project, piece: PieceId) -> (PointMm, PointMm) {
    if r.chance(12) {
        return (
            PointMm::new(wild_coord(r), wild_coord(r)),
            PointMm::new(wild_coord(r), wild_coord(r)),
        );
    }
    let Ok(_) = cur.piece(piece).and_then(|_| cur.piece_sheet(piece)) else {
        return (PointMm::new(0.0, 0.0), PointMm::new(1.0, 1.0));
    };
    let poly = piece_polygon(cur, piece);
    let (mut lo, mut hi) = (PointMm::new(1e9, 1e9), PointMm::new(-1e9, -1e9));
    for q in &poly {
        lo = PointMm::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = PointMm::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    let pt = |r: &mut Rng| {
        PointMm::new(
            lo.x - 10.0 + r.unit() * (hi.x - lo.x + 20.0),
            lo.y - 10.0 + r.unit() * (hi.y - lo.y + 20.0),
        )
    };
    match r.below(6) {
        // Verticale, horizontale, ou quelconque ; parfois deux points confondus.
        0 => {
            let x = lo.x + r.unit() * (hi.x - lo.x);
            (PointMm::new(x, lo.y - 5.0), PointMm::new(x, hi.y + 5.0))
        }
        1 => {
            let y = lo.y + r.unit() * (hi.y - lo.y);
            (PointMm::new(lo.x - 5.0, y), PointMm::new(hi.x + 5.0, y))
        }
        2 if r.chance(30) => {
            let a = pt(r);
            (a, a)
        }
        _ => (pt(r), pt(r)),
    }
}

/// Deux bords de longueurs voisines (un par pièce) : candidats plausibles à l'assemblage.
pub fn sj_matching_edges(cur: &Project, a: PieceId, b: PieceId) -> Option<(EntityId, EntityId)> {
    let (pa, pb) = (cur.piece(a).ok()?, cur.piece(b).ok()?);
    let (sa, sb) = (cur.piece_sheet(a).ok()?, cur.piece_sheet(b).ok()?);
    let mut best: Option<(f64, EntityId, EntityId)> = None;
    for ea in &pa.outline {
        for eb in &pb.outline {
            let (la, lb) = (
                sa.path_length_mm(ea.entity).ok()?,
                sb.path_length_mm(eb.entity).ok()?,
            );
            let gap = (la - lb).abs();
            if best.is_none_or(|(g, ..)| gap < g) {
                best = Some((gap, ea.entity, eb.entity));
            }
        }
    }
    best.map(|(_, x, y)| (x, y))
}

pub fn sj_tolerance(r: &mut Rng) -> f64 {
    match r.below(10) {
        0 => f64::NAN,
        1 => -1.0,
        2 => 1e9,
        3 => 0.0,
        4 => 10.0,
        5 => f64::INFINITY,
        _ => 0.01 + r.unit() * 2.0,
    }
}

/// Dimension tirée dans les bornes, sur les bornes, juste dehors, ou franchement hostile.
pub fn mk_size(r: &mut Rng, lo: f64, hi: f64) -> f64 {
    match r.below(10) {
        0..=5 => lo + r.unit() * (hi - lo),
        6 => [lo, hi][r.below(2)],
        7 => [lo / 2.0, hi + 0.01, 0.0][r.below(3)],
        8 => [
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1e300,
            f64::MIN_POSITIVE,
        ][r.below(6)],
        _ => {
            let e = extreme_numbers();
            e[r.below(e.len())]
        }
    }
}

/// Un point du tracé (interpolation d'un échantillon), ou un point quelconque.
pub fn ls_point_on(r: &mut Rng, s: &Sheet, e: EntityId) -> PointMm {
    match s.shape(e) {
        Ok(shape) => {
            let pts = shape.sample(8);
            pts[r.below(pts.len())]
        }
        Err(_) => pt_mm(r),
    }
}
