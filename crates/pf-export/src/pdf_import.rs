//! Lecteur PDF vectoriel (US-12.4) : **chemins seulement**. Le fichier est lu en lecture
//! linéaire des objets (`N G obj ... endobj`, flux compris), sans table `xref` : la page est
//! trouvée par `/Root` puis `/Kids`, ses flux de contenu sont décodés (aucun filtre, ou
//! `FlateDecode` seul) et leurs opérateurs `m l c v y h re` (et `cm q Q` pour la pile de
//! transformations) deviennent des formes ; `w` et tout le reste de la mise en forme sont ignorés.
//!
//! Aucun texte, aucune image, aucune police n'est lu : `BT`, `Do`, `sh`, images en ligne et
//! chemins de découpe sont **listés** dans le rapport. Un PDF chiffré, à flux d'objets compressés,
//! à filtre autre que Flate, tronqué ou incohérent est **refusé** avec un message clair : jamais
//! un import partiel muet. Unités : points (1/72 pouce) vers mm, multipliés par le `/UserUnit`
//! de la page (signalé en remarque). `/MediaBox` délimite la page et n'agit pas sur l'échelle.

use crate::vector_import::*;
use pf_core::MAX_IMPORT_OBJECTS;
use std::collections::{HashMap, HashSet};

const PT_MM: f64 = 25.4 / 72.0;
/// Taille maximale d'un flux une fois décompressé.
const MAX_STREAM_BYTES: usize = 128 * 1024 * 1024;
/// Budget de lecture du document entier : octets décodés de toutes les pages (un flux cité par
/// plusieurs pages compte à chaque fois).
const MAX_TOTAL_STREAM_BYTES: usize = 256 * 1024 * 1024;
/// Plus grand `/UserUnit` accepté (limite des lecteurs courants).
const MAX_USER_UNIT: f64 = 75_000.0;
const MAX_Q_DEPTH: usize = 256;
const MAX_NESTING: usize = 64;
const MAX_OPERANDS: usize = 200_000;
/// Plafonds de lecture d'une page : formes peintes, sommets des chemins, sortes différentes
/// d'opérateurs non lus (chacune tient une ligne du rapport).
const MAX_PAGE_SHAPES: usize = 2 * MAX_IMPORT_OBJECTS;
const MAX_PAGE_SEGMENTS: usize = 3 * MAX_IMPORT_OBJECTS;
const MAX_IGNORED_KINDS: usize = 10_000;

// ----- valeurs et lexique ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Null,
    Bool(#[allow(dead_code)] bool),
    Int(i64),
    Real(f64),
    Name(String),
    Str,
    Arr(Vec<Val>),
    Dict(Vec<(String, Val)>),
    Ref(u32),
    Op(String),
}

impl Val {
    fn num(&self) -> Option<f64> {
        match self {
            Val::Int(i) => Some(*i as f64),
            Val::Real(r) if r.is_finite() => Some(*r),
            _ => None,
        }
    }
    fn get(&self, key: &str) -> Option<&Val> {
        match self {
            Val::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
enum LexErr {
    Eof,
    Syntax,
}

fn is_pdf_ws(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

struct Lex<'a> {
    s: &'a [u8],
    i: usize,
    refs: bool,
}

fn parse_number(t: &[u8]) -> Option<Val> {
    let mut j = 0;
    if matches!(t.first(), Some(b'+' | b'-')) {
        j = 1;
    }
    let body = &t[j..];
    let dots = body.iter().filter(|b| **b == b'.').count();
    if body.is_empty() || dots > 1 || !body.iter().all(|b| b.is_ascii_digit() || *b == b'.') {
        return None;
    }
    if !body.iter().any(u8::is_ascii_digit) {
        return None;
    }
    let text = std::str::from_utf8(t).ok()?;
    if dots == 0 {
        if let Ok(i) = text.parse::<i64>() {
            return Some(Val::Int(i));
        }
    }
    let v: f64 = text.parse().ok()?;
    v.is_finite().then_some(Val::Real(v))
}

impl<'a> Lex<'a> {
    fn new(s: &'a [u8], refs: bool) -> Lex<'a> {
        Lex { s, i: 0, refs }
    }

    fn skip_ws(&mut self) {
        while let Some(&b) = self.s.get(self.i) {
            if is_pdf_ws(b) {
                self.i += 1;
            } else if b == b'%' {
                while self.i < self.s.len() && !matches!(self.s[self.i], b'\r' | b'\n') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn token(&mut self) -> &'a [u8] {
        let st = self.i;
        while self.i < self.s.len() && !is_pdf_ws(self.s[self.i]) && !is_delim(self.s[self.i]) {
            self.i += 1;
        }
        &self.s[st..self.i]
    }

    /// Valeur suivante ; `None` en fin de données.
    fn val(&mut self, depth: usize) -> Result<Option<Val>, LexErr> {
        if depth > MAX_NESTING {
            return Err(LexErr::Syntax);
        }
        self.skip_ws();
        let Some(&b) = self.s.get(self.i) else {
            return Ok(None);
        };
        match b {
            b'/' => {
                self.i += 1;
                let raw = self.token();
                let mut name = String::new();
                let mut k = 0;
                while k < raw.len() {
                    if raw[k] == b'#' && k + 2 < raw.len() {
                        let h = std::str::from_utf8(&raw[k + 1..k + 3]).ok();
                        if let Some(v) = h.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                            name.push(v as char);
                            k += 3;
                            continue;
                        }
                    }
                    name.push(raw[k] as char);
                    k += 1;
                }
                Ok(Some(Val::Name(name)))
            }
            b'(' => {
                self.i += 1;
                let mut depth_p = 1usize;
                while depth_p > 0 {
                    let Some(&c) = self.s.get(self.i) else {
                        return Err(LexErr::Eof);
                    };
                    self.i += 1;
                    match c {
                        b'\\' => self.i += 1,
                        b'(' => depth_p += 1,
                        b')' => depth_p -= 1,
                        _ => {}
                    }
                }
                Ok(Some(Val::Str))
            }
            b'<' if self.s.get(self.i + 1) == Some(&b'<') => {
                self.i += 2;
                let mut d = Vec::new();
                loop {
                    self.skip_ws();
                    if self.s.get(self.i..self.i + 2) == Some(b">>") {
                        self.i += 2;
                        return Ok(Some(Val::Dict(d)));
                    }
                    let key = match self.val(depth + 1)? {
                        Some(Val::Name(n)) => n,
                        Some(_) => return Err(LexErr::Syntax),
                        None => return Err(LexErr::Eof),
                    };
                    let v = match self.val(depth + 1)? {
                        Some(Val::Op(_)) | None => return Err(LexErr::Syntax),
                        Some(v) => v,
                    };
                    d.push((key, v));
                }
            }
            b'<' => {
                self.i += 1;
                while let Some(&c) = self.s.get(self.i) {
                    self.i += 1;
                    if c == b'>' {
                        return Ok(Some(Val::Str));
                    }
                    if !(c.is_ascii_hexdigit() || is_pdf_ws(c)) {
                        return Err(LexErr::Syntax);
                    }
                }
                Err(LexErr::Eof)
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                loop {
                    self.skip_ws();
                    match self.s.get(self.i) {
                        None => return Err(LexErr::Eof),
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Some(Val::Arr(a)));
                        }
                        _ => {}
                    }
                    match self.val(depth + 1)? {
                        Some(Val::Op(_)) | None => return Err(LexErr::Syntax),
                        Some(v) => a.push(v),
                    }
                    if a.len() > MAX_OPERANDS {
                        return Err(LexErr::Syntax);
                    }
                }
            }
            b')' | b'>' | b']' | b'{' | b'}' => Err(LexErr::Syntax),
            _ => {
                let t = self.token();
                if t.is_empty() {
                    return Err(LexErr::Syntax);
                }
                if let Some(n) = parse_number(t) {
                    if let (true, Val::Int(num)) = (self.refs, &n) {
                        if *num >= 0 {
                            let save = self.i;
                            self.skip_ws();
                            let g = self.token();
                            if matches!(parse_number(g), Some(Val::Int(_))) {
                                self.skip_ws();
                                if self.token() == b"R" {
                                    return Ok(Some(Val::Ref(*num as u32)));
                                }
                            }
                            self.i = save;
                        }
                    }
                    return Ok(Some(n));
                }
                Ok(Some(match t {
                    b"true" => Val::Bool(true),
                    b"false" => Val::Bool(false),
                    b"null" => Val::Null,
                    other => Val::Op(String::from_utf8_lossy(other).into_owned()),
                }))
            }
        }
    }
}

fn lex_err(e: LexErr) -> VectorImportError {
    match e {
        LexErr::Eof => VectorImportError::Truncated,
        LexErr::Syntax => VectorImportError::Structure("syntax"),
    }
}

fn find(s: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    if from >= s.len() {
        return None;
    }
    s[from..]
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + from)
}

// ----- objets ----------------------------------------------------------------------------------

struct Obj {
    val: Val,
    stream: Option<(usize, usize)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PdfDocument {
    /// Numéro d'objet de chaque page, dans l'ordre du document.
    pages: Vec<u32>,
    /// Contenu décodé de chaque page (concaténation de ses flux), même ordre.
    contents: Vec<Vec<u8>>,
    rotated: Vec<bool>,
    /// `/UserUnit` de chaque page : taille de son unité, en 1/72 de pouce (1 par défaut).
    user_units: Vec<f64>,
}

/// Numéro de l'objet dont l'en-tête « N G obj » se termine par le mot `obj` en `p` ; `None` si
/// ce n'en est pas un (par exemple `endobj`).
fn object_number(s: &[u8], p: usize) -> Option<u32> {
    if s.get(p + 3)
        .is_some_and(|b| !(is_pdf_ws(*b) || is_delim(*b)))
    {
        return None;
    }
    let mut j = p;
    let skip_ws = |j: &mut usize| {
        let e = *j;
        while *j > 0 && is_pdf_ws(s[*j - 1]) {
            *j -= 1;
        }
        *j < e
    };
    let digits = |j: &mut usize| {
        let e = *j;
        while *j > 0 && s[*j - 1].is_ascii_digit() {
            *j -= 1;
        }
        (*j, e)
    };
    if !skip_ws(&mut j) {
        return None;
    }
    let (g0, g1) = digits(&mut j);
    if g0 == g1 || !skip_ws(&mut j) {
        return None;
    }
    let (n0, n1) = digits(&mut j);
    if n0 == n1 {
        return None;
    }
    std::str::from_utf8(&s[n0..n1]).ok()?.parse().ok()
}

fn scan_objects(s: &[u8]) -> Result<HashMap<u32, Obj>, VectorImportError> {
    let mut objs = HashMap::new();
    let mut i = 0;
    while let Some(p) = find(s, b"obj", i) {
        let Some(num) = object_number(s, p) else {
            i = p + 3;
            continue;
        };
        let mut lx = Lex::new(s, true);
        lx.i = p + 3;
        let val = lx
            .val(0)
            .map_err(lex_err)?
            .ok_or(VectorImportError::Truncated)?;
        if matches!(val, Val::Op(_)) {
            return Err(VectorImportError::Structure("object"));
        }
        lx.skip_ws();
        let mut stream = None;
        if s[lx.i..].starts_with(b"stream") {
            let mut st = lx.i + 6;
            if s.get(st) == Some(&b'\r') && s.get(st + 1) == Some(&b'\n') {
                st += 2;
            } else if matches!(s.get(st), Some(b'\n' | b'\r')) {
                st += 1;
            }
            let declared = match val.get("Length") {
                Some(Val::Int(l)) if *l >= 0 => Some(*l as usize),
                _ => None,
            };
            let ok_end = declared.and_then(|l| {
                let e = st.checked_add(l)?;
                let mut k = e;
                while k < s.len() && is_pdf_ws(s[k]) {
                    k += 1;
                }
                s.get(k..)?.starts_with(b"endstream").then_some(e)
            });
            let end = match ok_end {
                Some(e) => e,
                None => find(s, b"endstream", st).ok_or(VectorImportError::Truncated)?,
            };
            stream = Some((st, end));
            lx.i = end;
        }
        i = lx.i.max(p + 3);
        objs.insert(num, Obj { val, stream });
    }
    Ok(objs)
}

fn resolve<'a>(objs: &'a HashMap<u32, Obj>, v: &'a Val) -> &'a Val {
    let mut cur = v;
    for _ in 0..16 {
        match cur {
            Val::Ref(n) => match objs.get(n) {
                Some(o) => cur = &o.val,
                None => return &Val::Null,
            },
            _ => return cur,
        }
    }
    &Val::Null
}

/// Décode un flux et le décompte de `budget` : au moins sa taille dans le fichier, pour que des
/// milliers de références à un flux qui ne décompresse rien restent bornées elles aussi.
fn decode_stream(
    s: &[u8],
    objs: &HashMap<u32, Obj>,
    o: &Obj,
    budget: &mut usize,
) -> Result<Vec<u8>, VectorImportError> {
    let (st, end) = o.stream.ok_or(VectorImportError::Structure("contents"))?;
    let raw = &s[st..end.min(s.len())];
    let filter = o.val.get("Filter").map(|f| resolve(objs, f));
    let flate = match filter {
        None | Some(Val::Null) => false,
        Some(Val::Name(n)) if n == "FlateDecode" || n == "Fl" => true,
        Some(Val::Arr(a)) if a.is_empty() => false,
        Some(Val::Arr(a)) if a.len() == 1 => match &a[0] {
            Val::Name(n) if n == "FlateDecode" || n == "Fl" => true,
            _ => return Err(VectorImportError::Unsupported("filter")),
        },
        Some(_) => return Err(VectorImportError::Unsupported("filter")),
    };
    if let Some(p) = o.val.get("DecodeParms").map(|p| resolve(objs, p)) {
        let predictor = match p {
            Val::Dict(_) => p.get("Predictor").and_then(Val::num).unwrap_or(1.0),
            Val::Null => 1.0,
            _ => return Err(VectorImportError::Unsupported("predictor")),
        };
        if predictor > 1.0 {
            return Err(VectorImportError::Unsupported("predictor"));
        }
    }
    // `+ 1` : même vide, une référence coûte l'octet de séparation.
    if raw.len() >= *budget {
        return Err(VectorImportError::Unsupported("total_too_big"));
    }
    let data = if !flate {
        raw.to_vec()
    } else {
        let limit = MAX_STREAM_BYTES.min(*budget);
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(raw, limit).map_err(|e| {
            if e.status != miniz_oxide::inflate::TINFLStatus::HasMoreOutput {
                VectorImportError::Structure("flate")
            } else if limit < MAX_STREAM_BYTES {
                VectorImportError::Unsupported("total_too_big")
            } else {
                VectorImportError::Unsupported("too_big")
            }
        })?
    };
    *budget = budget.saturating_sub(data.len().max(raw.len()) + 1);
    Ok(data)
}

impl PdfDocument {
    pub fn parse(bytes: &[u8]) -> Result<PdfDocument, VectorImportError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(VectorImportError::TooLarge { bytes: bytes.len() });
        }
        let head = &bytes[..bytes.len().min(1024)];
        if find(head, b"%PDF-", 0).is_none() {
            return Err(VectorImportError::NotPdf);
        }
        let objs = scan_objects(bytes)?;
        // Chiffrement et flux d'objets : refusés avant toute lecture de page.
        let mut root: Option<Val> = None;
        let mut at = 0;
        while let Some(p) = find(bytes, b"trailer", at) {
            let mut lx = Lex::new(bytes, true);
            lx.i = p + 7;
            if let Ok(Some(d @ Val::Dict(_))) = lx.val(0) {
                if d.get("Encrypt").is_some() {
                    return Err(VectorImportError::Encrypted);
                }
                if let Some(r) = d.get("Root") {
                    root = Some(r.clone());
                }
            }
            at = p + 7;
        }
        for o in objs.values() {
            match o.val.get("Type") {
                Some(Val::Name(n)) if n == "ObjStm" => {
                    return Err(VectorImportError::Unsupported("object_streams"));
                }
                Some(Val::Name(n)) if n == "XRef" => {
                    if o.val.get("Encrypt").is_some() {
                        return Err(VectorImportError::Encrypted);
                    }
                    if root.is_none() {
                        root = o.val.get("Root").cloned();
                    }
                }
                _ => {}
            }
        }
        let root = root.ok_or(VectorImportError::Truncated)?;
        let catalog = resolve(&objs, &root);
        let pages_root = catalog
            .get("Pages")
            .ok_or(VectorImportError::Structure("catalog"))?;
        let mut pages = Vec::new();
        let mut seen = HashSet::new();
        collect_pages(&objs, pages_root, None, &mut pages, &mut seen, 0)?;
        if pages.is_empty() {
            return Err(VectorImportError::NoPage);
        }
        let mut contents = Vec::with_capacity(pages.len());
        let mut rotated = Vec::with_capacity(pages.len());
        let mut nums = Vec::with_capacity(pages.len());
        let mut user_units = Vec::with_capacity(pages.len());
        let mut budget = MAX_TOTAL_STREAM_BYTES;
        for (num, page, rotate) in &pages {
            nums.push(*num);
            rotated.push(
                rotate
                    .map(|r| resolve(&objs, r))
                    .and_then(Val::num)
                    .is_some_and(|r| r.rem_euclid(360.0) != 0.0),
            );
            // Propre à la page (non héritable). Illisible = échelle inconnue : refus.
            user_units.push(match page.get("UserUnit") {
                None | Some(Val::Null) => 1.0,
                Some(v) => resolve(&objs, v)
                    .num()
                    .filter(|u| *u > 0.0 && *u <= MAX_USER_UNIT)
                    .ok_or(VectorImportError::Structure("UserUnit"))?,
            });
            let mut data = Vec::new();
            let c = page.get("Contents").map(|c| resolve(&objs, c));
            let list: Vec<&Val> = match c {
                Some(Val::Arr(a)) => a.iter().collect(),
                Some(Val::Dict(_)) => page.get("Contents").into_iter().collect(),
                _ => Vec::new(),
            };
            for r in list {
                let Val::Ref(n) = r else {
                    return Err(VectorImportError::Structure("contents"));
                };
                let o = objs
                    .get(n)
                    .ok_or(VectorImportError::Structure("contents"))?;
                data.extend(decode_stream(bytes, &objs, o, &mut budget)?);
                data.push(b'\n');
                if data.len() > MAX_STREAM_BYTES {
                    return Err(VectorImportError::Unsupported("too_big"));
                }
            }
            contents.push(data);
        }
        Ok(PdfDocument {
            pages: nums,
            contents,
            rotated,
            user_units,
        })
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Aperçu de la page `page` (à partir de 1) : millimètres = points × 25,4 / 72 × `scale`.
    pub fn build(&self, page: usize, scale: f64) -> Result<VectorBuild, VectorImportError> {
        check_scale(scale)?;
        if page == 0 || page > self.pages.len() {
            return Err(VectorImportError::PageOutOfRange {
                page,
                pages: self.pages.len(),
            });
        }
        let mut stats = ReadStats::default();
        if self.rotated[page - 1] {
            stats.note(NoteKind::PageRotation);
        }
        let user_unit = self.user_units[page - 1];
        if user_unit != 1.0 {
            stats.note(NoteKind::UserUnit);
        }
        let shapes = interpret(&self.contents[page - 1], &mut stats)?;
        let k = PT_MM * user_unit * scale;
        build_shapes(
            &shapes,
            stats,
            Matrix::new(k, 0.0, 0.0, k, 0.0, 0.0),
            k,
            DEFAULT_TOLERANCE_MM,
        )
    }
}

/// Pages dans l'ordre du document, chacune avec son `/Rotate` propre ou hérité d'un parent.
fn collect_pages<'a>(
    objs: &'a HashMap<u32, Obj>,
    node_ref: &'a Val,
    inherited_rotate: Option<&'a Val>,
    out: &mut Vec<(u32, &'a Val, Option<&'a Val>)>,
    seen: &mut HashSet<u32>,
    depth: usize,
) -> Result<(), VectorImportError> {
    if depth > MAX_NESTING || out.len() > 100_000 {
        return Err(VectorImportError::Structure("page tree"));
    }
    let num = match node_ref {
        Val::Ref(n) => *n,
        _ => return Err(VectorImportError::Structure("page tree")),
    };
    if !seen.insert(num) {
        return Err(VectorImportError::Structure("page tree"));
    }
    let node = resolve(objs, node_ref);
    let rotate = node.get("Rotate").or(inherited_rotate);
    match node.get("Kids").map(|k| resolve(objs, k)) {
        Some(Val::Arr(kids)) => {
            for k in kids {
                collect_pages(objs, k, rotate, out, seen, depth + 1)?;
            }
            Ok(())
        }
        Some(_) => Err(VectorImportError::Structure("page tree")),
        None => {
            if matches!(node, Val::Dict(_)) {
                out.push((num, node, rotate));
                Ok(())
            } else {
                Err(VectorImportError::Structure("page tree"))
            }
        }
    }
}

// ----- flux de contenu -------------------------------------------------------------------------

fn nums<const N: usize>(ops: &[Val]) -> Result<[f64; N], VectorImportError> {
    if ops.len() != N {
        return Err(VectorImportError::Structure("operands"));
    }
    let mut out = [0.0; N];
    for (slot, v) in out.iter_mut().zip(ops) {
        *slot = v.num().ok_or(VectorImportError::Structure("operands"))?;
    }
    Ok(out)
}

/// Refus dès qu'une page demande plus que ce qu'un import accepte.
fn check_page_caps(
    shapes: usize,
    segments: usize,
    stats: &ReadStats,
) -> Result<(), VectorImportError> {
    for (count, max) in [
        (shapes, MAX_PAGE_SHAPES),
        (segments, MAX_PAGE_SEGMENTS),
        (stats.ignored.len(), MAX_IGNORED_KINDS),
    ] {
        if count > max {
            return Err(VectorImportError::TooManyElements { max });
        }
    }
    Ok(())
}

fn interpret(content: &[u8], stats: &mut ReadStats) -> Result<Vec<ShapeEntry>, VectorImportError> {
    let mut lx = Lex::new(content, false);
    let mut operands: Vec<Val> = Vec::new();
    let mut shapes = Vec::new();
    let mut ctm = Matrix::IDENTITY;
    let mut saved: Vec<Matrix> = Vec::new();
    let mut path: Vec<Seg> = Vec::new();
    // Sommets des chemins déjà retenus dans `shapes`.
    let mut kept_segments = 0usize;
    let (mut cur, mut start): (Option<P2>, P2) = (None, (0.0, 0.0));
    let mut clip = false;
    let bad = || VectorImportError::Structure("operands");
    loop {
        let v = match lx.val(0) {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(e) => return Err(lex_err(e)),
        };
        let Val::Op(op) = v else {
            operands.push(v);
            if operands.len() > MAX_OPERANDS {
                return Err(VectorImportError::Structure("operands"));
            }
            continue;
        };
        let ops = std::mem::take(&mut operands);
        check_page_caps(shapes.len(), kept_segments + path.len(), stats)?;
        match op.as_str() {
            "q" => {
                if saved.len() >= MAX_Q_DEPTH {
                    return Err(VectorImportError::Structure("q depth"));
                }
                saved.push(ctm);
            }
            "Q" => ctm = saved.pop().ok_or(VectorImportError::Structure("Q"))?,
            "cm" => {
                let [a, b, c, d, e, f] = nums(&ops)?;
                let m = ctm.compose(Matrix::new(a, b, c, d, e, f));
                if !m.is_finite() {
                    return Err(bad());
                }
                ctm = m;
            }
            "m" => {
                let [x, y] = nums(&ops)?;
                path.push(Seg::M((x, y)));
                cur = Some((x, y));
                start = (x, y);
            }
            "l" => {
                let [x, y] = nums(&ops)?;
                if cur.is_none() {
                    return Err(bad());
                }
                path.push(Seg::L((x, y)));
                cur = Some((x, y));
            }
            "c" | "v" | "y" => {
                let c0 = cur.ok_or_else(bad)?;
                let (c1, c2, p) = match op.as_str() {
                    "c" => {
                        let [a, b, c, d, e, f] = nums(&ops)?;
                        ((a, b), (c, d), (e, f))
                    }
                    "v" => {
                        let [c, d, e, f] = nums(&ops)?;
                        (c0, (c, d), (e, f))
                    }
                    _ => {
                        let [a, b, e, f] = nums(&ops)?;
                        ((a, b), (e, f), (e, f))
                    }
                };
                path.push(Seg::C(c1, c2, p));
                cur = Some(p);
            }
            "h" => {
                if cur.is_some() {
                    path.push(Seg::Z);
                    cur = Some(start);
                }
            }
            "re" => {
                let [x, y, w, h] = nums(&ops)?;
                path.push(Seg::M((x, y)));
                path.push(Seg::L((x + w, y)));
                path.push(Seg::L((x + w, y + h)));
                path.push(Seg::L((x, y + h)));
                path.push(Seg::Z);
                cur = Some((x, y));
                start = (x, y);
            }
            "W" | "W*" => clip = true,
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                if !ops.is_empty() {
                    return Err(bad());
                }
                if matches!(op.as_str(), "s" | "b" | "b*") && cur.is_some() {
                    path.push(Seg::Z);
                }
                if !path.is_empty() {
                    if clip {
                        stats.note(NoteKind::ClipPathDropped);
                    } else {
                        kept_segments += path.len();
                        shapes.push(ShapeEntry {
                            tag: "path".into(),
                            geom: Ok(ShapeGeom::Path(std::mem::take(&mut path))),
                            ctm,
                            raised: false,
                            snapped: false,
                        });
                    }
                }
                path.clear();
                cur = None;
                clip = false;
            }
            "BT" => stats.ignore("text", IgnoreReason::Unsupported),
            "Do" => stats.ignore("xobject", IgnoreReason::Unsupported),
            "sh" => stats.ignore("shading", IgnoreReason::Unsupported),
            "BI" => {
                stats.ignore("image", IgnoreReason::Unsupported);
                loop {
                    match lx.val(0) {
                        Ok(Some(Val::Op(o))) if o == "ID" => break,
                        Ok(Some(_)) => {}
                        Ok(None) => return Err(VectorImportError::Truncated),
                        Err(e) => return Err(lex_err(e)),
                    }
                }
                let s = lx.s;
                let mut k = lx.i;
                loop {
                    let Some(p) = find(s, b"EI", k) else {
                        return Err(VectorImportError::Truncated);
                    };
                    let before = p == 0 || is_pdf_ws(s[p - 1]);
                    let after = s.get(p + 2).is_none_or(|b| is_pdf_ws(*b));
                    if before && after {
                        lx.i = p + 2;
                        break;
                    }
                    k = p + 2;
                }
            }
            // Mise en forme, texte (ses opérateurs suivent un `BT` déjà compté), contenu balisé.
            "w" | "J" | "j" | "M" | "d" | "i" | "ri" | "gs" | "g" | "G" | "rg" | "RG" | "k"
            | "K" | "cs" | "CS" | "sc" | "SC" | "scn" | "SCN" | "BMC" | "BDC" | "EMC" | "MP"
            | "DP" | "BX" | "EX" | "d0" | "d1" | "ET" | "Tf" | "Td" | "TD" | "Tm" | "T*" | "Tj"
            | "TJ" | "'" | "\"" | "Tc" | "Tw" | "Tz" | "TL" | "Tr" | "Ts" => {}
            other => stats.ignore(&format!("op {other}"), IgnoreReason::Unsupported),
        }
    }
    // Le dernier opérateur peut avoir franchi un plafond.
    check_page_caps(shapes.len(), kept_segments + path.len(), stats)?;
    if !path.is_empty() {
        stats.ignore("path", IgnoreReason::Unterminated);
    }
    Ok(shapes)
}
