//! Projets des tests du rapport de contrôle (US-08.5) : scénario à valeurs calculées **à la main**
//! et générateur de projets aléatoires à graine fixe.
#![allow(dead_code)]

use pf_core::*;
use pf_geometry::{PointMm, Side};

pub fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Polygone en segments (`ents[k]` va de `v[k]` à `v[k+1]`), extrait en pièce ; sommets
/// anti-horaires.
pub fn poly(p: &mut Project, s: SheetId, v: &[(f64, f64)], name: &str) -> (PieceId, Vec<EntityId>) {
    let n = v.len();
    let ents: Vec<EntityId> = (0..n)
        .map(|k| {
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(v[k].0, v[k].1)),
                to: PointRef::New(pt(v[(k + 1) % n].0, v[(k + 1) % n].1)),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: ents.clone(),
            },
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (piece, ents)
}

pub fn notch(p: &mut Project, s: SheetId, e: EntityId, distance_mm: f64) -> NotchId {
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm,
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap()
    .notches[0]
}

pub fn declare(
    p: &mut Project,
    a: (PieceId, EntityId),
    b: (PieceId, EntityId),
    embu: Option<f64>,
    tol: Option<f64>,
) -> SeamPairId {
    let sa = p.seam_side_range(a.0, a.1, 1).unwrap();
    let sb = p.seam_side_range(b.0, b.1, 1).unwrap();
    p.apply(Command::AddSeamPair {
        a: sa,
        b: sb,
        embu_mm: embu,
        tolerance_mm: tol,
    })
    .unwrap()
    .seam_pairs[0]
}

/// Scénario de référence (valeurs calculées à la main, écrites en dur dans les tests) :
///
/// - A « Devant » : rectangle 50 × 110 ; bord cousu = côté droit `(50,0) → (50,110)`, 110 mm ;
///   aire 5500 mm², périmètre 320 mm.
/// - B « Dos » : quadrilatère `(100,100) (100,0) (150, 50·tan 10°) (150,100)` ; bord cousu = côté
///   gauche `(100,100) → (100,0)`, 100 mm ; angle intérieur de 80° en `(100,0)` ; aire
///   4779,591275 mm², périmètre 291,954982 mm.
/// - couple A/B, embu attendu 10 mm (110 − 100 = 10 : écart nul), tolérance par défaut 1 mm ;
/// - crans : A à 22 mm et 55 mm du début, B à 50 mm du début ; le cran A 55 mm correspond au cran
///   B 50 mm (55 × 100 / 110), le cran A 22 mm n'a pas de vis-à-vis (attendu à 80 mm de B) ;
/// - raccords : début de A (90°) + fin de B (80°) = 170°, écart −10° (cassé, creux) ; fin de A
///   (90°) + début de B (90°) = 180° (lisse).
pub struct Ca1 {
    pub p: Project,
    pub sheet: SheetId,
    pub a: PieceId,
    pub b: PieceId,
    pub ea: Vec<EntityId>,
    pub eb: Vec<EntityId>,
    pub pair: SeamPairId,
    pub n_a22: NotchId,
    pub n_a55: NotchId,
    pub n_b50: NotchId,
}

pub fn ca1() -> Ca1 {
    let mut p = Project::new("Veste");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (50.0, 0.0), (50.0, 110.0), (0.0, 110.0)],
        "Devant",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[
            (100.0, 100.0),
            (100.0, 0.0),
            (150.0, 8.816349),
            (150.0, 100.0),
        ],
        "Dos",
    );
    let pair = declare(&mut p, (a, ea[1]), (b, eb[0]), Some(10.0), None);
    let n_a22 = notch(&mut p, s, ea[1], 22.0);
    let n_a55 = notch(&mut p, s, ea[1], 55.0);
    let n_b50 = notch(&mut p, s, eb[0], 50.0);
    Ca1 {
        p,
        sheet: s,
        a,
        b,
        ea,
        eb,
        pair,
        n_a22,
        n_a55,
        n_b50,
    }
}

// ---------------------------------------------------------------------------------------------
// Générateur aléatoire à graine fixe
// ---------------------------------------------------------------------------------------------

/// Générateur congruentiel (graine fixe, aucune dépendance).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() % 10_000) as f64 / 10_000.0
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// Textes à caractères spéciaux (CSV : guillemets, `;`, `=` en tête, retours ligne ; PDF :
/// accents, mots très longs, caractères absents de la police).
pub const SPECIAL: &[&str] = &[
    "Devant",
    "Dos \"large\"",
    "=SOMME(A1)",
    "+33 6",
    "-tiret",
    "@mention",
    "Manche; gauche",
    "Éléonore Œuvre àéîôù",
    "Pièce très longue sans espace aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "Doublure, côté 'gauche' (n°2)",
    "日本語 ✓ emoji 😀",
    "a  b   c",
    "x",
];

fn text(r: &mut Rng) -> String {
    SPECIAL[r.below(SPECIAL.len() as u64) as usize].to_string()
}

/// Projet aléatoire : polygones convexes, couples, crans, déplacements, suppressions, fiches
/// riches. Toujours valide (une commande refusée est simplement ignorée).
pub fn random_project(seed: u64) -> Project {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5);
    let mut p = Project::new(
        &text(&mut r)
            .chars()
            .take(100)
            .collect::<String>()
            .replace(|c: char| c.is_control(), " "),
    );
    if p.name.trim().is_empty() {
        p.name = "Projet".into();
    }
    if r.chance(20) {
        p.name = "=1+1;\"x\" ligne".into();
    }
    let s = p.sheets[0].id;
    let n_pieces = 1 + r.below(5) as usize;
    let mut pieces: Vec<(PieceId, Vec<EntityId>)> = Vec::new();
    for k in 0..n_pieces {
        let x0 = 200.0 * k as f64;
        let w = r.range(30.0, 120.0).round();
        let h = r.range(30.0, 120.0).round();
        let sides = 3 + r.below(3) as usize;
        let v: Vec<(f64, f64)> = match sides {
            3 => vec![(x0, 0.0), (x0 + w, 0.0), (x0 + w / 2.0, h)],
            4 => vec![(x0, 0.0), (x0 + w, 0.0), (x0 + w, h), (x0, h)],
            _ => vec![
                (x0, 0.0),
                (x0 + w, 0.0),
                (x0 + w, h * 0.6),
                (x0 + w / 2.0, h),
                (x0, h * 0.6),
            ],
        };
        let (piece, ents) = poly_quiet(&mut p, s, &v, &text(&mut r));
        if let Some(piece) = piece {
            p.apply(Command::SetPieceCard {
                piece,
                name: text(&mut r)
                    .chars()
                    .take(100)
                    .collect::<String>()
                    .replace(|c: char| c.is_control(), " "),
                code: r.chance(60).then(|| text(&mut r)),
                quantity: 1 + r.below(4) as u32,
                mirrored_pair: r.chance(30),
                material: r.chance(60).then(|| text(&mut r)),
                category: match r.below(7) {
                    0 => Some(PieceCategory::Fabric),
                    1 => Some(PieceCategory::Lining),
                    2 => Some(PieceCategory::Interfacing),
                    3 => Some(PieceCategory::Leather),
                    4 => Some(PieceCategory::Reinforcement),
                    _ => None,
                },
                comment: r.chance(30).then(|| text(&mut r)),
            })
            .ok();
            pieces.push((piece, ents));
        }
    }
    // Couples : arêtes distinctes (une arête n'appartient qu'à un couple).
    let mut used: Vec<EntityId> = Vec::new();
    let n_pairs = r.below(5) as usize;
    for _ in 0..n_pairs {
        if pieces.is_empty() {
            break;
        }
        let ia = r.below(pieces.len() as u64) as usize;
        let ib = r.below(pieces.len() as u64) as usize;
        let ea = pieces[ia].1[r.below(pieces[ia].1.len() as u64) as usize];
        let eb = pieces[ib].1[r.below(pieces[ib].1.len() as u64) as usize];
        if ea == eb || used.contains(&ea) || used.contains(&eb) {
            continue;
        }
        let (Ok(sa), Ok(sb)) = (
            p.seam_side_range(pieces[ia].0, ea, 1),
            p.seam_side_range(pieces[ib].0, eb, 1),
        ) else {
            continue;
        };
        let embu = r.chance(50).then(|| r.range(0.0, 20.0));
        let tol = r.chance(30).then(|| r.range(0.0, 5.0));
        if p.apply(Command::AddSeamPair {
            a: sa,
            b: sb,
            embu_mm: embu,
            tolerance_mm: tol,
        })
        .is_ok()
        {
            used.push(ea);
            used.push(eb);
        }
    }
    // Crans sur les arêtes cousues.
    for e in used.clone() {
        for _ in 0..r.below(3) {
            let len = p.sheet(s).ok().and_then(|sh| sh.path_length_mm(e).ok());
            if let Some(len) = len.filter(|l| *l > 4.0) {
                let d = r.range(1.0, len - 1.0);
                p.apply(Command::AddNotch {
                    sheet: s,
                    entity: e,
                    distance_mm: d,
                    side: Side::Left,
                    length_mm: 5.0,
                    style: NotchStyle::Single,
                })
                .ok();
            }
        }
    }
    // Contour rendu ouvert (un tracé libre retiré) : les couples de la pièce deviennent non
    // calculables sans disparaître.
    if r.chance(30) && !pieces.is_empty() {
        let piece = pieces[r.below(pieces.len() as u64) as usize].0;
        if let Ok(pc) = p.piece(piece) {
            let mut outline = pc.outline.clone();
            if let Some(k) = outline.iter().rposition(|e| !used.contains(&e.entity)) {
                outline.remove(k);
                p.apply(Command::SetPieceOutline {
                    piece,
                    outline,
                    grainline: None,
                })
                .ok();
            }
        }
    }
    // Déplacements de points, puis suppressions.
    let point_ids: Vec<PointId> = p
        .sheet(s)
        .map(|sh| sh.points.iter().map(|q| q.id).collect())
        .unwrap_or_default();
    for _ in 0..r.below(4) {
        if point_ids.is_empty() {
            break;
        }
        let id = point_ids[r.below(point_ids.len() as u64) as usize];
        p.apply(Command::MovePoint {
            sheet: s,
            point: id,
            to: pt(r.range(-50.0, 500.0), r.range(-50.0, 150.0)),
        })
        .ok();
    }
    if r.chance(25) && !pieces.is_empty() {
        let k = r.below(pieces.len() as u64) as usize;
        p.apply(Command::DeletePiece { piece: pieces[k].0 }).ok();
    }
    if r.chance(15) {
        if let Some(e) = used.first() {
            p.apply(Command::Delete {
                sheet: s,
                points: Vec::new(),
                entities: vec![*e],
            })
            .ok();
        }
    }
    p
}

fn poly_quiet(
    p: &mut Project,
    s: SheetId,
    v: &[(f64, f64)],
    name: &str,
) -> (Option<PieceId>, Vec<EntityId>) {
    let n = v.len();
    let ents: Vec<EntityId> = (0..n)
        .filter_map(|k| {
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(v[k].0, v[k].1)),
                to: PointRef::New(pt(v[(k + 1) % n].0, v[(k + 1) % n].1)),
            })
            .ok()
            .map(|o| o.entities[0])
        })
        .collect();
    let clean: String = name.chars().filter(|c| !c.is_control()).take(100).collect();
    let name = if clean.trim().is_empty() {
        "P".into()
    } else {
        clean
    };
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: ents.clone(),
            },
            name,
            detached: false,
        })
        .ok()
        .and_then(|o| o.pieces.first().copied());
    (piece, ents)
}
