//! Lecteur DXF **indépendant** de l'écrivain (US-12.1) : il lit un DXF texte par paires
//! code / valeur et reconstruit les entités. Il ne réutilise rien de `pf_export::dxf` (ni
//! constantes, ni table de couches, ni fonction) : les nombres de décimales, les noms de couches
//! et les versions sont lus dans le fichier. Il refuse tout ce qui n'est pas une structure DXF
//! valide (paires incomplètes, code non entier, nombre non fini, sections déséquilibrées, EOF
//! absent).
#![allow(dead_code)]

use std::collections::BTreeMap;

pub type P = (f64, f64);

#[derive(Debug, Clone, PartialEq)]
pub enum Ent {
    Line {
        layer: String,
        a: P,
        b: P,
    },
    Point {
        layer: String,
        p: P,
    },
    Circle {
        layer: String,
        c: P,
        r: f64,
    },
    Polyline {
        layer: String,
        pts: Vec<P>,
        closed: bool,
    },
    Text {
        layer: String,
        at: P,
        height: f64,
        text: String,
    },
    Insert {
        layer: String,
        block: String,
        at: P,
    },
}

impl Ent {
    pub fn layer(&self) -> &str {
        match self {
            Ent::Line { layer, .. }
            | Ent::Point { layer, .. }
            | Ent::Circle { layer, .. }
            | Ent::Polyline { layer, .. }
            | Ent::Text { layer, .. }
            | Ent::Insert { layer, .. } => layer,
        }
    }
}

#[derive(Debug, Default)]
pub struct Dxf {
    /// Variables d'en-tête : nom → paires qui suivent.
    pub header: BTreeMap<String, Vec<(i32, String)>>,
    pub ltypes: Vec<String>,
    /// Couches de la table : (nom, couleur).
    pub layers: Vec<(String, i32)>,
    /// Blocs dans l'ordre du fichier.
    pub blocks: Vec<(String, Vec<Ent>)>,
    pub entities: Vec<Ent>,
}

struct Rec {
    kind: String,
    pairs: Vec<(i32, String)>,
}

fn real(v: &str) -> Result<f64, String> {
    let x: f64 = v
        .trim()
        .parse()
        .map_err(|_| format!("nombre illisible « {v} »"))?;
    if x.is_finite() {
        Ok(x)
    } else {
        Err(format!("nombre non fini « {v} »"))
    }
}

fn get(pairs: &[(i32, String)], code: i32) -> Option<&str> {
    pairs
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, v)| v.as_str())
}

fn num(pairs: &[(i32, String)], code: i32) -> Result<f64, String> {
    real(get(pairs, code).ok_or_else(|| format!("code {code} absent"))?)
}

fn pt(pairs: &[(i32, String)], base: i32) -> Result<P, String> {
    Ok((num(pairs, base)?, num(pairs, base + 10)?))
}

/// Décode `\U+XXXX` (unités UTF-16) ; tout le reste est pris tel quel.
pub fn decode_text(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut units: Vec<u16> = Vec::new();
    let mut out = String::new();
    let flush = |units: &mut Vec<u16>, out: &mut String| {
        if !units.is_empty() {
            out.push_str(&String::from_utf16_lossy(units));
            units.clear();
        }
    };
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\'
            && i + 7 <= b.len()
            && b.get(i + 1) == Some(&'U')
            && b.get(i + 2) == Some(&'+')
        {
            let hex: String = b[i + 3..i + 7].iter().collect();
            if let Ok(u) = u16::from_str_radix(&hex, 16) {
                units.push(u);
                i += 7;
                continue;
            }
        }
        flush(&mut units, &mut out);
        out.push(b[i]);
        i += 1;
    }
    flush(&mut units, &mut out);
    out
}

fn records(text: &str) -> Result<Vec<Rec>, String> {
    let lines: Vec<&str> = text.split('\n').collect();
    // Le fichier se termine par un saut de ligne : dernier élément vide.
    if lines.last() != Some(&"") {
        return Err("le fichier ne se termine pas par un saut de ligne".into());
    }
    let lines = &lines[..lines.len() - 1];
    if lines.len() % 2 != 0 {
        return Err("nombre impair de lignes : paire code / valeur incomplète".into());
    }
    let mut recs: Vec<Rec> = Vec::new();
    for (i, chunk) in lines.chunks(2).enumerate() {
        let code: i32 = chunk[0]
            .trim()
            .parse()
            .map_err(|_| format!("paire {i} : code « {} » non entier", chunk[0]))?;
        let value = chunk[1].trim_end_matches('\r').to_string();
        if code == 0 {
            recs.push(Rec {
                kind: value,
                pairs: Vec::new(),
            });
        } else {
            recs.last_mut()
                .ok_or("le fichier ne commence pas par le code 0")?
                .pairs
                .push((code, value));
        }
    }
    Ok(recs)
}

fn entity(rec: &Rec, vertices: Vec<P>, closed: bool) -> Result<Ent, String> {
    let layer = get(&rec.pairs, 8)
        .ok_or_else(|| format!("{} sans couche", rec.kind))?
        .to_string();
    Ok(match rec.kind.as_str() {
        "LINE" => Ent::Line {
            layer,
            a: pt(&rec.pairs, 10)?,
            b: pt(&rec.pairs, 11)?,
        },
        "POINT" => Ent::Point {
            layer,
            p: pt(&rec.pairs, 10)?,
        },
        "CIRCLE" => Ent::Circle {
            layer,
            c: pt(&rec.pairs, 10)?,
            r: num(&rec.pairs, 40)?,
        },
        "TEXT" => Ent::Text {
            layer,
            at: pt(&rec.pairs, 10)?,
            height: num(&rec.pairs, 40)?,
            text: decode_text(get(&rec.pairs, 1).ok_or("TEXT sans chaîne")?),
        },
        "POLYLINE" => Ent::Polyline {
            layer,
            pts: vertices,
            closed,
        },
        "INSERT" => Ent::Insert {
            layer,
            block: get(&rec.pairs, 2).ok_or("INSERT sans bloc")?.to_string(),
            at: pt(&rec.pairs, 10)?,
        },
        other => return Err(format!("entité inconnue {other}")),
    })
}

/// Lit une suite d'entités jusqu'au premier enregistrement de fin (`ENDSEC` ou `ENDBLK`) ;
/// renvoie les entités et l'indice de l'enregistrement de fin.
fn entities(recs: &[Rec], mut i: usize, end: &str) -> Result<(Vec<Ent>, usize), String> {
    let mut out = Vec::new();
    while i < recs.len() && recs[i].kind != end {
        let r = &recs[i];
        if r.kind == "POLYLINE" {
            let flag = num(&r.pairs, 70)? as i64;
            if get(&r.pairs, 66) != Some("1") {
                return Err("POLYLINE sans 66=1".into());
            }
            let mut verts = Vec::new();
            i += 1;
            while i < recs.len() && recs[i].kind == "VERTEX" {
                verts.push(pt(&recs[i].pairs, 10)?);
                if get(&recs[i].pairs, 8) != get(&r.pairs, 8) {
                    return Err("sommet sur une autre couche que sa polyligne".into());
                }
                i += 1;
            }
            if i >= recs.len() || recs[i].kind != "SEQEND" {
                return Err("POLYLINE sans SEQEND".into());
            }
            out.push(entity(r, verts, flag & 1 == 1)?);
        } else {
            out.push(entity(r, Vec::new(), false)?);
        }
        i += 1;
    }
    if i >= recs.len() {
        return Err(format!("{end} absent"));
    }
    Ok((out, i))
}

pub fn parse(text: &str) -> Result<Dxf, String> {
    if !text.is_ascii() {
        return Err("le fichier contient des octets non ASCII".into());
    }
    let recs = records(text)?;
    if recs.last().map(|r| r.kind.as_str()) != Some("EOF") {
        return Err("EOF absent en dernière position".into());
    }
    let mut dxf = Dxf::default();
    let mut i = 0;
    while recs[i].kind != "EOF" {
        if recs[i].kind != "SECTION" {
            return Err(format!("« {} » hors section", recs[i].kind));
        }
        let name = get(&recs[i].pairs, 2)
            .ok_or("SECTION sans nom")?
            .to_string();
        match name.as_str() {
            "HEADER" => {
                let mut cur: Option<String> = None;
                for (c, v) in recs[i].pairs.iter().skip(1) {
                    if *c == 9 {
                        cur = Some(v.clone());
                        dxf.header.insert(v.clone(), Vec::new());
                    } else if let Some(k) = &cur {
                        dxf.header.get_mut(k).unwrap().push((*c, v.clone()));
                    } else {
                        return Err("valeur d'en-tête sans variable".into());
                    }
                }
                i += 1;
            }
            "TABLES" => {
                i += 1;
                while recs[i].kind == "TABLE" {
                    let table = get(&recs[i].pairs, 2).ok_or("TABLE sans nom")?.to_string();
                    let declared = num(&recs[i].pairs, 70)? as usize;
                    i += 1;
                    let mut n = 0;
                    while recs[i].kind != "ENDTAB" {
                        let name = get(&recs[i].pairs, 2).ok_or("entrée sans nom")?.to_string();
                        match (table.as_str(), recs[i].kind.as_str()) {
                            ("LTYPE", "LTYPE") => dxf.ltypes.push(name),
                            ("LAYER", "LAYER") => {
                                dxf.layers.push((name, num(&recs[i].pairs, 62)? as i32))
                            }
                            (t, k) => return Err(format!("entrée {k} dans la table {t}")),
                        }
                        n += 1;
                        i += 1;
                    }
                    if n != declared {
                        return Err(format!("table {table} : {declared} déclarées, {n} lues"));
                    }
                    i += 1;
                }
                if recs[i].kind != "ENDSEC" {
                    return Err("TABLES sans ENDSEC".into());
                }
                i += 1;
            }
            "BLOCKS" => {
                i += 1;
                while recs[i].kind == "BLOCK" {
                    let bname = get(&recs[i].pairs, 2).ok_or("BLOCK sans nom")?.to_string();
                    let (ents, end) = entities(&recs, i + 1, "ENDBLK")?;
                    dxf.blocks.push((bname, ents));
                    i = end + 1;
                }
                if recs[i].kind != "ENDSEC" {
                    return Err("BLOCKS sans ENDSEC".into());
                }
                i += 1;
            }
            "ENTITIES" => {
                let (ents, end) = entities(&recs, i + 1, "ENDSEC")?;
                dxf.entities = ents;
                i = end + 1;
            }
            other => return Err(format!("section inconnue {other}")),
        }
        if i >= recs.len() {
            return Err("fin de fichier inattendue".into());
        }
        if name == "HEADER" {
            if recs[i].kind != "ENDSEC" {
                return Err("HEADER sans ENDSEC".into());
            }
            i += 1;
        }
    }
    Ok(dxf)
}

impl Dxf {
    /// Valeur entière d'une variable d'en-tête (code 70).
    pub fn header_int(&self, name: &str) -> Option<i64> {
        get(self.header.get(name)?, 70)?.trim().parse().ok()
    }

    pub fn header_str(&self, name: &str) -> Option<String> {
        Some(get(self.header.get(name)?, 1)?.to_string())
    }

    pub fn header_point(&self, name: &str) -> Option<P> {
        pt(self.header.get(name)?, 10).ok()
    }

    /// Entités de chaque bloc **inséré** dans `ENTITIES`, dans l'ordre des insertions ; toute
    /// insertion doit viser un bloc existant, à l'origine.
    pub fn inserted(&self) -> Result<Vec<(&str, &[Ent])>, String> {
        let mut out = Vec::new();
        for e in &self.entities {
            let Ent::Insert { block, at, .. } = e else {
                return Err(format!("entité {e:?} directement dans ENTITIES"));
            };
            if at.0 != 0.0 || at.1 != 0.0 {
                return Err("INSERT hors de l'origine".into());
            }
            let (name, ents) = self
                .blocks
                .iter()
                .find(|(n, _)| n == block)
                .ok_or_else(|| format!("bloc {block} introuvable"))?;
            out.push((name.as_str(), ents.as_slice()));
        }
        Ok(out)
    }
}

// ----- géométrie de contrôle (écrite ici) -----------------------------------------------------

pub fn area(pts: &[P]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

pub fn perimeter(pts: &[P], closed: bool) -> f64 {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            (b.0 - a.0).hypot(b.1 - a.1)
        })
        .sum()
}

pub fn bbox(pts: &[P]) -> (P, P) {
    let mut lo = (f64::INFINITY, f64::INFINITY);
    let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    (lo, hi)
}

pub fn dist(a: P, b: P) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Distance d'un point à la polyligne (fermée ou non).
pub fn dist_to_polyline(p: P, pts: &[P], closed: bool) -> f64 {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 {
                0.0
            } else {
                (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
            };
            dist(p, (a.0 + t * dx, a.1 + t * dy))
        })
        .fold(f64::INFINITY, f64::min)
}
