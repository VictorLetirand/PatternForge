//! US-12.3 — campagne adverse du DXF générique : 160 séquences à graine fixe.
//!
//! Chaque séquence :
//!
//! (a) **aller-retour** : une feuille aléatoire (segments, arcs des deux sens, cercles, Béziers,
//!     courbes lisses, points isolés, rôles, calques) est exportée, relue, importée dans un projet
//!     neuf : géométrie conservée à 0,01 mm (segments, arcs et cercles à 1e-4 mm), couches
//!     attendues, **un seul pas d'annulation** dont l'annulation redonne les octets d'avant ;
//! (b) **fichiers corrompus** : l'export et le fichier d'un tiers sont mutés (troncature,
//!     lignes supprimées, valeurs aberrantes, codes de groupe invalides, lignes échangées ou
//!     dupliquées, octets de bruit) : jamais de panique, soit refus propre, soit lecture dont le
//!     rapport est cohérent (chaque entité lue est importée ou ignorée) ; l'import dans un projet
//!     est accepté (projet valide) ou refusé **sans trace** (octets identiques) ;
//! (c) **unités aléatoires** : boîte englobante en mm = boîte native × facteur ;
//! (d) **déterminisme** : lecture, conversion, export et import donnent les mêmes octets.
//!
//! L'oracle est indépendant du code testé (`common/dxf_generic.rs`). Ceci ne prouve pas la
//! lecture de fichiers de logiciels réels.

mod common;

use common::dxf_generic::*;
use common::Rng;
use pf_core::{persist, Command, Document, ImportItem, PathRole, Project};
use pf_export::dxf_construction::{export_construction_dxf, ConstructionError, ConstructionScope};
use pf_export::dxf_import::{DxfDocument, DxfUnit, UnitChoice};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

const SEQUENCES: u64 = 160;

static ROUND_TRIPS: AtomicUsize = AtomicUsize::new(0);
static EMPTY_SHEETS: AtomicUsize = AtomicUsize::new(0);
static WITH_CURVES: AtomicUsize = AtomicUsize::new(0);
static WITH_ROLES: AtomicUsize = AtomicUsize::new(0);
static MUTATED: AtomicUsize = AtomicUsize::new(0);
static MUTATED_REFUSED: AtomicUsize = AtomicUsize::new(0);
static MUTATED_IMPORTED: AtomicUsize = AtomicUsize::new(0);
static MUTATED_COMMAND_REFUSED: AtomicUsize = AtomicUsize::new(0);
static UNIT_CHECKS: AtomicUsize = AtomicUsize::new(0);
static POISONED: AtomicUsize = AtomicUsize::new(0);

macro_rules! ck {
    ($seq:expr, $cond:expr, $($arg:tt)+) => {
        if !$cond {
            panic!("séquence {} : {}", $seq, format!($($arg)+));
        }
    };
}

const SLUGS: [&str; 14] = [
    "construction",
    "sewing",
    "cutting",
    "grain",
    "fold",
    "axis",
    "topstitch",
    "center_front",
    "center_back",
    "waist",
    "hip",
    "bust",
    "skiving",
    "glue",
];

fn slug(role: PathRole) -> &'static str {
    SLUGS[PathRole::ALL.iter().position(|r| *r == role).unwrap()]
}

fn ex_layer(e: &Ex) -> &str {
    match e {
        Ex::Line { layer, .. }
        | Ex::Point { layer, .. }
        | Ex::Circle { layer, .. }
        | Ex::Arc { layer, .. }
        | Ex::Poly { layer, .. } => layer,
    }
}

/// Couches attendues, calculées depuis le modèle (sans le code d'export).
fn expected_layers(p: &Project, s: pf_core::SheetId) -> Vec<String> {
    let sheet = p.sheet(s).unwrap();
    let name_of = |l: Option<pf_core::LayerId>| match l {
        Some(id) => sheet.layer(id).unwrap().name.clone(),
        None => "0".to_string(),
    };
    let mut out = Vec::new();
    let mut used = std::collections::HashSet::new();
    for e in &sheet.entities {
        used.extend(e.kind.all_points());
        let base = name_of(e.layer);
        out.push(if e.role == PathRole::Construction {
            base
        } else {
            format!("{base}.{}", slug(e.role))
        });
    }
    for pt in &sheet.points {
        if !used.contains(&pt.id) {
            out.push(name_of(pt.layer));
        }
    }
    out.sort();
    out
}

fn arc_bbox(c: (f64, f64), r: f64, s: (f64, f64), e: (f64, f64), out: &mut Vec<(f64, f64)>) {
    // Arc anti-horaire de s à e : extrémités, plus les axes que le balayage traverse.
    out.push(s);
    out.push(e);
    let a0 = (s.1 - c.1).atan2(s.0 - c.0).to_degrees().rem_euclid(360.0);
    let a1 = (e.1 - c.1).atan2(e.0 - c.0).to_degrees().rem_euclid(360.0);
    let sweep = (a1 - a0).rem_euclid(360.0);
    for k in 0..4 {
        let axis = f64::from(k) * 90.0;
        if (axis - a0).rem_euclid(360.0) <= sweep {
            out.push((
                c.0 + r * axis.to_radians().cos(),
                c.1 + r * axis.to_radians().sin(),
            ));
        }
    }
}

fn native_bbox(sum: &Summary) -> Option<((f64, f64), (f64, f64))> {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for (a, b) in &sum.segments {
        pts.push(*a);
        pts.push(*b);
    }
    for (c, r) in &sum.circles {
        pts.push((c.0 - r, c.1 - r));
        pts.push((c.0 + r, c.1 + r));
    }
    for (c, r, s, e) in &sum.arcs {
        arc_bbox(*c, *r, *s, *e, &mut pts);
    }
    pts.extend(sum.points.iter().copied());
    let first = *pts.first()?;
    let mut mn = first;
    let mut mx = first;
    for p in pts {
        mn = (mn.0.min(p.0), mn.1.min(p.1));
        mx = (mx.0.max(p.0), mx.1.max(p.1));
    }
    Some((mn, mx))
}

// ----- mutations -------------------------------------------------------------------------------

fn garbage(rng: &mut Rng) -> String {
    rng.pick(&[
        "NaN",
        "inf",
        "-inf",
        "1e999",
        "-1e308",
        "1e308",
        "abc",
        "",
        "  ",
        "0x10",
        "1,5",
        "--3",
        "999999999999999999999",
        "1e-320",
        "\u{7f}",
        "\\U+ZZZZ",
        "-0",
        "+5",
    ])
    .to_string()
}

fn mutate(rng: &mut Rng, text: &str) -> Vec<u8> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut bytes_noise = false;
    for _ in 0..1 + rng.below(3) {
        if lines.len() < 8 {
            break;
        }
        let n = lines.len();
        match rng.below(9) {
            0 => lines.truncate(rng.below(n as u64) as usize),
            1 => {
                for _ in 0..1 + rng.below(6) {
                    if lines.len() > 4 {
                        let i = rng.below(lines.len() as u64) as usize;
                        lines.remove(i);
                    }
                }
            }
            2 => {
                let i = (rng.below((n / 2) as u64) as usize) * 2 + 1;
                lines[i] = garbage(rng);
            }
            3 => {
                let i = (rng.below((n / 2) as u64) as usize) * 2;
                lines[i] = rng
                    .pick(&["x", "-9", "5000", "1072", "", "  0 0", "9999"])
                    .to_string();
            }
            4 => {
                let i = (rng.below((n / 2) as u64) as usize) * 2;
                let (a, b) = (lines[i].clone(), lines[i + 1].clone());
                lines.insert(i, b);
                lines.insert(i, a);
            }
            5 => {
                let (i, j) = (rng.below(n as u64) as usize, rng.below(n as u64) as usize);
                lines.swap(i, j);
            }
            6 => bytes_noise = true,
            7 => {
                // Valeurs numériques extrêmes à la place de vraies coordonnées.
                for _ in 0..1 + rng.below(4) {
                    let i = (rng.below((n / 2) as u64) as usize) * 2 + 1;
                    if lines[i].trim().parse::<f64>().is_ok() {
                        lines[i] = rng
                            .pick(&["1e30", "-1e30", "1e-12", "7e5", "-3e5", "0", "360", "-360"])
                            .to_string();
                    }
                }
            }
            _ => {
                // Une entité recopiée : doublons.
                let i = (rng.below((n / 2) as u64) as usize) * 2;
                let j = (i + 6).min(n);
                let chunk: Vec<String> = lines[i..j].to_vec();
                for (k, l) in chunk.into_iter().enumerate() {
                    lines.insert(i + k, l);
                }
            }
        }
    }
    let mut out = lines.join("\n").into_bytes();
    out.push(b'\n');
    if bytes_noise && !out.is_empty() {
        for _ in 0..1 + rng.below(4) {
            let i = rng.below(out.len() as u64) as usize;
            out[i] = rng.below(256) as u8;
        }
    }
    out
}

fn check_mutated(seq: u64, rng: &mut Rng, bytes: &[u8]) {
    MUTATED.fetch_add(1, Relaxed);
    let doc = match DxfDocument::parse(bytes) {
        Ok(d) => d,
        Err(_) => {
            MUTATED_REFUSED.fetch_add(1, Relaxed);
            return;
        }
    };
    for _ in 0..2 {
        let unit = rng.pick(&DxfUnit::CHOICES);
        let Ok(b) = doc.build(unit) else { continue };
        let read: usize = b.report.read.values().sum();
        let imported: usize = b.report.imported.values().sum();
        ck!(
            seq,
            imported + b.report.ignored_total() == read,
            "rapport incohérent : {imported} importées + {} ignorées ≠ {read} lues",
            b.report.ignored_total()
        );
        ck!(
            seq,
            b.report.objects == b.objects.len(),
            "nombre d'objets du rapport"
        );
        let layer_objects: usize = b.report.layers.iter().map(|l| l.objects).sum();
        ck!(seq, layer_objects == b.objects.len(), "objets par couche");
        if b.objects.is_empty() {
            ck!(seq, b.bbox_mm.is_none(), "boîte sans objet");
            continue;
        }
        let bb = b.bbox_mm.unwrap();
        ck!(
            seq,
            bb.min.x.is_finite() && bb.max.y.is_finite() && bb.min.x <= bb.max.x,
            "boîte non finie"
        );
        // La validation du projet est quadratique : au-delà de 2 500 objets, on n'importe qu'un
        // préfixe (la limite complète est éprouvée par `dxf_generique.rs`).
        let objects: Vec<pf_core::ImportObject> = b.objects.iter().take(2500).cloned().collect();
        let mut doc = Document::new(Project::new("cible"));
        let s = doc.project().sheets[0].id;
        let before = doc.project().clone();
        let json_before = persist::to_json(&before).unwrap();
        match doc.execute(Command::ImportGeometry {
            sheet: s,
            objects: objects.clone(),
        }) {
            Ok(out) => {
                MUTATED_IMPORTED.fetch_add(1, Relaxed);
                ck!(
                    seq,
                    doc.project().validate().is_ok(),
                    "projet invalide après import"
                );
                ck!(
                    seq,
                    out.entities.len() <= objects.len(),
                    "plus de tracés que d'objets"
                );
                ck!(seq, doc.project().pieces.is_empty(), "pièce inventée");
                ck!(
                    seq,
                    doc.undo() && !doc.can_undo(),
                    "un seul pas d'annulation"
                );
                ck!(
                    seq,
                    persist::to_json(doc.project()).unwrap() == json_before,
                    "annulation : octets différents"
                );
            }
            Err(e) => {
                MUTATED_COMMAND_REFUSED.fetch_add(1, Relaxed);
                ck!(seq, *doc.project() == before, "refus avec trace : {e}");
                ck!(seq, !doc.can_undo(), "étape d'historique sur un refus");
            }
        }
        // Empoisonnement : un objet reçoit une coordonnée invalide : la commande refuse, sans
        // trace.
        if rng.below(3) == 0 {
            let mut poisoned = objects;
            let i = rng.below(poisoned.len() as u64) as usize;
            let bad = rng.pick(&[f64::NAN, f64::INFINITY, 2.0e6, -1.0e30]);
            poisoned[i].item = match &poisoned[i].item {
                ImportItem::Point { .. } => ImportItem::Point { at: pm(bad, 0.0) },
                ImportItem::Segment { from, .. } => ImportItem::Segment {
                    from: *from,
                    to: pm(0.0, bad),
                },
                ImportItem::Circle { center, .. } => ImportItem::Circle {
                    center: *center,
                    radius_mm: bad,
                },
                ImportItem::Arc {
                    center, start, ccw, ..
                } => ImportItem::Arc {
                    center: *center,
                    start: *start,
                    end: pm(bad, 1.0),
                    ccw: *ccw,
                },
                ImportItem::Spline { points } => {
                    let mut pts = points.clone();
                    pts[0] = pm(bad, bad);
                    ImportItem::Spline { points: pts }
                }
                ImportItem::Bezier { start, c1, end, .. } => ImportItem::Bezier {
                    start: *start,
                    c1: *c1,
                    c2: pm(bad, 0.0),
                    end: *end,
                },
            };
            let mut p = Project::new("cible");
            let sheet = p.sheets[0].id;
            let before = p.clone();
            let json_before = persist::to_json(&p).unwrap();
            ck!(
                seq,
                p.apply(Command::ImportGeometry {
                    sheet,
                    objects: poisoned,
                })
                .is_err(),
                "objet invalide accepté"
            );
            ck!(seq, p == before, "refus avec trace");
            ck!(
                seq,
                persist::to_json(&p).unwrap() == json_before,
                "octets changés"
            );
            POISONED.fetch_add(1, Relaxed);
        }
    }
}

// ----- la campagne -----------------------------------------------------------------------------

fn run(seq: u64) {
    let mut rng = Rng(0x7C3B_1203_D0F5_99E1 ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for _ in 0..4 {
        rng.next();
    }
    let (p, s) = random_project(&mut rng);
    let before = p.clone();
    let orig = summarize(&p, s);
    let exported = export_construction_dxf(&p, ConstructionScope::Sheet(s));
    ck!(seq, p == before, "l'export a modifié le projet");
    let exp = match exported {
        Ok(e) => e,
        Err(ConstructionError::NothingToExport) => {
            EMPTY_SHEETS.fetch_add(1, Relaxed);
            ck!(
                seq,
                p.sheet(s).unwrap().entities.is_empty(),
                "refus d'une feuille non vide"
            );
            // Un fichier muté reste testé sur la fixture d'un tiers.
            let fixture = String::from_utf8(tiers(Some(4))).unwrap();
            let m = mutate(&mut rng, &fixture);
            check_mutated(seq, &mut rng, &m);
            return;
        }
        Err(e) => panic!("séquence {seq} : export refusé : {e}"),
    };
    ROUND_TRIPS.fetch_add(1, Relaxed);
    WITH_CURVES.fetch_add(usize::from(orig.curves > 0), Relaxed);
    WITH_ROLES.fetch_add(
        usize::from(
            p.sheet(s)
                .unwrap()
                .entities
                .iter()
                .any(|e| e.role != PathRole::Construction),
        ),
        Relaxed,
    );

    // Structure du fichier et couches (relecture indépendante).
    let f = read_export(&exp.text).unwrap_or_else(|e| panic!("séquence {seq} : {e}"));
    ck!(seq, exp.text.is_ascii(), "export non ASCII");
    ck!(
        seq,
        !exp.text.to_lowercase().contains("nan") && !exp.text.contains("inf"),
        "nombre non fini"
    );
    let mut got: Vec<String> = f.entities.iter().map(|e| ex_layer(e).to_string()).collect();
    got.sort();
    ck!(seq, got == expected_layers(&p, s), "couches : {got:?}");
    ck!(
        seq,
        exp.report.total_entities() == f.entities.len(),
        "rapport d'export"
    );

    // (a) aller-retour.
    let doc = DxfDocument::parse(exp.bytes()).unwrap_or_else(|e| panic!("séquence {seq} : {e}"));
    let build = doc
        .build_choice(UnitChoice::Declared)
        .unwrap_or_else(|e| panic!("séquence {seq} : {e}"));
    ck!(
        seq,
        build.report.ignored.is_empty(),
        "entités ignorées : {:?}",
        build.report.ignored
    );
    ck!(seq, build.scale == 1.0, "unité déclarée = mm");
    let mut doc_q = Document::new(Project::new("relu"));
    let qs = doc_q.project().sheets[0].id;
    let json_before = persist::to_json(doc_q.project()).unwrap();
    doc_q
        .execute(Command::ImportGeometry {
            sheet: qs,
            objects: build.objects.clone(),
        })
        .unwrap_or_else(|e| panic!("séquence {seq} : import refusé : {e}"));
    let imp = summarize(doc_q.project(), qs);
    if let Err(m) = compare(&orig, &imp, 0.01) {
        panic!("séquence {seq} : {m}");
    }
    ck!(seq, doc_q.undo(), "pas d'annulation");
    ck!(
        seq,
        persist::to_json(doc_q.project()).unwrap() == json_before,
        "annulation : octets différents"
    );
    ck!(seq, !doc_q.can_undo(), "plus d'un pas d'annulation");

    // (c) unités aléatoires, sur les feuilles sans courbe (boîte exacte).
    if orig.curves == 0 {
        if let Some((mn, mx)) = native_bbox(&orig) {
            // Facteurs écrits en dur (millimètres par unité) : pas ceux du code testé.
            let (unit, k) = rng.pick(&[
                (DxfUnit::Millimetre, 1.0),
                (DxfUnit::Centimetre, 10.0),
                (DxfUnit::Metre, 1000.0),
                (DxfUnit::Inch, 25.4),
                (DxfUnit::Foot, 304.8),
            ]);
            let b = doc.build(unit).unwrap();
            ck!(
                seq,
                b.scale == k,
                "{unit:?} : facteur {} au lieu de {k}",
                b.scale
            );
            let bb = b.bbox_mm.unwrap();
            let tol = 1e-5 * k + 1e-6;
            ck!(
                seq,
                (bb.min.x - mn.0 * k).abs() < tol
                    && (bb.min.y - mn.1 * k).abs() < tol
                    && (bb.max.x - mx.0 * k).abs() < tol
                    && (bb.max.y - mx.1 * k).abs() < tol,
                "{unit:?} : boîte {bb:?} contre {mn:?} {mx:?} × {k}"
            );
            UNIT_CHECKS.fetch_add(1, Relaxed);
        }
    }

    // (d) déterminisme.
    let again = export_construction_dxf(&p, ConstructionScope::Sheet(s)).unwrap();
    ck!(seq, again.bytes() == exp.bytes(), "export non déterministe");
    let build2 = DxfDocument::parse(again.bytes())
        .unwrap()
        .build_choice(UnitChoice::Declared)
        .unwrap();
    ck!(seq, build2 == build, "lecture non déterministe");
    let mut q2 = Project::new("relu");
    let q2s = q2.sheets[0].id;
    q2.apply(Command::ImportGeometry {
        sheet: q2s,
        objects: build2.objects,
    })
    .unwrap();
    let mut q1 = Project::new("relu");
    let q1s = q1.sheets[0].id;
    q1.apply(Command::ImportGeometry {
        sheet: q1s,
        objects: build.objects,
    })
    .unwrap();
    ck!(
        seq,
        persist::to_json(&q1).unwrap() == persist::to_json(&q2).unwrap(),
        "import non déterministe"
    );

    // (b) fichiers corrompus : l'export, et la fixture d'un tiers une séquence sur deux.
    let mut bases = vec![exp.text.clone()];
    if seq % 2 == 0 {
        bases.push(String::from_utf8(tiers(Some(4))).unwrap());
    }
    for base in bases {
        for _ in 0..2 {
            let m = mutate(&mut rng, &base);
            check_mutated(seq, &mut rng, &m);
        }
    }
}

#[test]
fn campagne_adverse_160_sequences() {
    for seq in 0..SEQUENCES {
        let result = std::panic::catch_unwind(|| run(seq));
        if let Err(e) = result {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            if msg.starts_with("séquence") {
                panic!("{msg}");
            }
            panic!("séquence {seq} : panique inattendue : {msg}");
        }
    }
    // La campagne a réellement exercé ce qu'elle prétend exercer.
    let (rt, empty) = (ROUND_TRIPS.load(Relaxed), EMPTY_SHEETS.load(Relaxed));
    assert_eq!(rt + empty, SEQUENCES as usize);
    assert!(rt > 140, "aller-retour : {rt}");
    assert!(
        WITH_CURVES.load(Relaxed) > 40,
        "courbes : {}",
        WITH_CURVES.load(Relaxed)
    );
    assert!(
        WITH_ROLES.load(Relaxed) > 30,
        "rôles : {}",
        WITH_ROLES.load(Relaxed)
    );
    assert!(
        UNIT_CHECKS.load(Relaxed) > 30,
        "unités : {}",
        UNIT_CHECKS.load(Relaxed)
    );
    let (m, refused, imported) = (
        MUTATED.load(Relaxed),
        MUTATED_REFUSED.load(Relaxed),
        MUTATED_IMPORTED.load(Relaxed),
    );
    assert!(m >= 400, "fichiers mutés : {m}");
    assert!(refused > 40, "refus propres : {refused}");
    assert!(imported > 100, "imports cohérents : {imported}");
    assert!(
        POISONED.load(Relaxed) > 40,
        "imports empoisonnés : {}",
        POISONED.load(Relaxed)
    );
    eprintln!(
        "campagne : {rt} allers-retours ({} avec courbes, {} avec rôles), {empty} feuilles vides, \
         {m} fichiers mutés ({refused} refusés, {imported} importés, {} refusés par la commande), \
         {} contrôles d'unité, {} imports empoisonnés refusés",
        WITH_CURVES.load(Relaxed),
        WITH_ROLES.load(Relaxed),
        MUTATED_COMMAND_REFUSED.load(Relaxed),
        UNIT_CHECKS.load(Relaxed),
        POISONED.load(Relaxed)
    );
}
