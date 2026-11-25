//! Lecture indépendante du texte d'un PDF PatternForge (US-06.6).
//!
//! Le texte est écrit en CID hexadécimaux (`<0123…> Tj`, police composite `Identity-H`). Ce
//! lecteur ne réutilise rien du code d'export : il lit la table `ToUnicode` **du fichier** et
//! décode chaque chaîne. `readable` rend le PDF avec des chaînes lisibles `(texte) Tj`, pour
//! que les anciens contrôles qui cherchent un texte dans un flux de page continuent à
//! fonctionner.
#![allow(dead_code)]

use std::collections::HashMap;

/// Table CID → texte lue dans les blocs `beginbfchar` du flux `ToUnicode`.
pub fn to_unicode(pdf: &str) -> HashMap<u16, String> {
    let mut map = HashMap::new();
    let mut inside = false;
    for line in pdf.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.ends_with("beginbfchar") {
            inside = true;
        } else if line == "endbfchar" {
            inside = false;
        } else if inside {
            let mut it = line.split_whitespace();
            let (Some(a), Some(b)) = (it.next(), it.next()) else {
                continue;
            };
            let cid = u16::from_str_radix(a.trim_matches(['<', '>']), 16).expect("CID hexadécimal");
            let units: Vec<u16> = b
                .trim_matches(['<', '>'])
                .as_bytes()
                .chunks(4)
                .map(|q| u16::from_str_radix(std::str::from_utf8(q).unwrap(), 16).unwrap())
                .collect();
            map.insert(cid, String::from_utf16(&units).expect("UTF-16 valide"));
        }
    }
    map
}

/// Décode une chaîne hexadécimale de CID (4 chiffres par caractère) ; un CID absent de la
/// table `ToUnicode` donne « \u{FFFD} » (le contrôle qui le lit échouera).
pub fn decode_hex(map: &HashMap<u16, String>, hex: &str) -> String {
    hex.as_bytes()
        .chunks(4)
        .map(|q| {
            let cid = u16::from_str_radix(std::str::from_utf8(q).unwrap(), 16).unwrap();
            map.get(&cid).cloned().unwrap_or_else(|| "\u{FFFD}".into())
        })
        .collect()
}

/// Le PDF avec chaque `<hex> Tj ET` remplacé par `(texte) Tj ET` (parenthèses et antislash
/// échappés). Les autres lignes, y compris les fins de ligne CR LF, sont conservées.
pub fn readable(bytes: &[u8]) -> String {
    let text = std::str::from_utf8(bytes).expect("PDF ASCII");
    let map = to_unicode(text);
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match (line.ends_with(" Tj ET"), line.rfind('<'), line.rfind('>')) {
            (true, Some(a), Some(b)) if a < b => {
                out.push_str(&line[..a]);
                out.push('(');
                for ch in decode_hex(&map, &line[a + 1..b]).chars() {
                    if matches!(ch, '(' | ')' | '\\') {
                        out.push('\\');
                    }
                    out.push(ch);
                }
                out.push(')');
                out.push_str(&line[b + 1..]);
            }
            _ => out.push_str(line),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Lecteurs indépendants : police TrueType et objets de police du PDF
// ---------------------------------------------------------------------------------------------

use std::collections::BTreeMap;

fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}

fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Police TrueType lue par un lecteur de test (largeurs d'avance et `cmap` de format 12).
pub struct Ttf {
    pub upem: u16,
    pub num_glyphs: u16,
    pub tags: Vec<String>,
    adv: Vec<u16>,
    /// Longueur en octets des données de chaque glyphe (0 = glyphe vide).
    pub glyph_len: Vec<u32>,
    pub cmap: BTreeMap<u32, u16>,
}

pub fn parse_ttf(f: &[u8]) -> Ttf {
    let n = usize::from(be16(f, 4));
    let mut t: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for i in 0..n {
        let r = 12 + 16 * i;
        let tag = String::from_utf8_lossy(&f[r..r + 4]).to_string();
        t.insert(tag, (be32(f, r + 8) as usize, be32(f, r + 12) as usize));
    }
    let head = t["head"].0;
    let upem = be16(f, head + 18);
    let num_glyphs = be16(f, t["maxp"].0 + 4);
    let nhm = usize::from(be16(f, t["hhea"].0 + 34));
    let hmtx = t["hmtx"].0;
    let adv = (0..usize::from(num_glyphs))
        .map(|g| be16(f, hmtx + 4 * g.min(nhm - 1)))
        .collect();
    let loca = t["loca"].0;
    let long = be16(f, head + 50) != 0;
    let offset = |g: usize| -> u32 {
        if long {
            be32(f, loca + 4 * g)
        } else {
            u32::from(be16(f, loca + 2 * g)) * 2
        }
    };
    let glyph_len = (0..usize::from(num_glyphs))
        .map(|g| offset(g + 1) - offset(g))
        .collect();
    let mut cmap = BTreeMap::new();
    if let Some((cm, _)) = t.get("cmap").copied() {
        for i in 0..usize::from(be16(f, cm + 2)) {
            let sub = cm + be32(f, cm + 8 + 8 * i) as usize;
            if be16(f, sub) == 12 {
                for g in 0..be32(f, sub + 12) as usize {
                    let o = sub + 16 + 12 * g;
                    let (a, b, s) = (be32(f, o), be32(f, o + 4), be32(f, o + 8));
                    for c in a..=b {
                        cmap.insert(c, (s + c - a) as u16);
                    }
                }
            }
        }
    }
    Ttf {
        upem,
        num_glyphs,
        tags: t.keys().cloned().collect(),
        adv,
        glyph_len,
        cmap,
    }
}

impl Ttf {
    /// Largeur d'avance (unités) du glyphe d'un caractère de la police.
    pub fn advance_of(&self, ch: char) -> Option<u16> {
        self.cmap
            .get(&(ch as u32))
            .map(|g| self.adv[usize::from(*g)])
    }

    pub fn advance_of_gid(&self, gid: u16) -> u16 {
        self.adv[usize::from(gid)]
    }

    /// Largeur (mm) d'un texte de corps `size_pt` ; un caractère absent compte comme « ? ».
    pub fn width_mm(&self, text: &str, size_pt: f64) -> f64 {
        let units: u32 = text
            .chars()
            .map(|c| {
                u32::from(
                    self.advance_of(c)
                        .or_else(|| self.advance_of('?'))
                        .expect("« ? » dans la police"),
                )
            })
            .sum();
        f64::from(units) / f64::from(self.upem) * size_pt * 25.4 / 72.0
    }
}

/// Police livrée dans le dépôt (`assets/fonts/NotoSans-Regular-subset.ttf`), lue sur disque.
pub fn repo_font_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/NotoSans-Regular-subset.ttf"
    );
    std::fs::read(path).expect("police du dépôt")
}

pub fn repo_font() -> Ttf {
    parse_ttf(&repo_font_bytes())
}

/// Objets de police d'un PDF, lus par la table `xref` du fichier.
pub struct PdfFont {
    pub type0: String,
    pub cid_font: String,
    pub descriptor: String,
    pub base_font: String,
    pub font_file: Vec<u8>,
    pub cid_to_gid: Vec<u16>,
    pub widths: HashMap<u16, i64>,
    pub to_unicode: HashMap<u16, String>,
}

/// Corps de l'objet `n` (de `n 0 obj` à `endobj`) d'après la table `xref`.
pub fn object(pdf: &str, n: usize) -> String {
    let sx = pdf.rfind("startxref\n").expect("startxref");
    let xref: usize = pdf[sx + 10..].lines().next().unwrap().parse().unwrap();
    let entry = pdf[xref..].lines().nth(2 + n).expect("entrée xref");
    let off: usize = entry[..10].parse().unwrap();
    let body = &pdf[off..];
    let head = format!("{n} 0 obj\n");
    assert!(body.starts_with(&head), "objet {n} mal référencé");
    let end = body.find("\nendobj\n").expect("endobj");
    body[head.len()..end].to_string()
}

fn reference(dict: &str, key: &str) -> usize {
    let at = dict
        .find(key)
        .unwrap_or_else(|| panic!("{key} absent de {dict}"));
    let rest = dict[at + key.len()..].trim_start().trim_start_matches('[');
    rest.split_whitespace().next().unwrap().parse().unwrap()
}

/// Données d'un flux : contenu entre `stream\r\n` et `\nendstream`, décodé si `ASCIIHexDecode`.
pub fn stream_data(obj: &str) -> Vec<u8> {
    let start = obj.find("stream\r\n").expect("flux à fin CR LF") + 8;
    let end = obj.rfind("\nendstream").expect("endstream");
    let data = &obj[start..end];
    if obj.contains("/ASCIIHexDecode") {
        assert!(data.ends_with('>'), "marque de fin du filtre hexadécimal");
        let hex: String = data
            .chars()
            .take_while(|c| *c != '>')
            .filter(|c| !c.is_whitespace())
            .collect();
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    } else {
        data.as_bytes().to_vec()
    }
}

pub fn read_font(pdf_bytes: &[u8]) -> PdfFont {
    let pdf = std::str::from_utf8(pdf_bytes).expect("PDF ASCII");
    let type0 = object(pdf, 3);
    assert!(type0.contains("/Subtype /Type0") && type0.contains("/Encoding /Identity-H"));
    let cid_font = object(pdf, reference(&type0, "/DescendantFonts"));
    assert!(cid_font.contains("/Subtype /CIDFontType2"));
    let descriptor = object(pdf, reference(&cid_font, "/FontDescriptor"));
    let font_file = stream_data(&object(pdf, reference(&descriptor, "/FontFile2")));
    let gid_bytes = stream_data(&object(pdf, reference(&cid_font, "/CIDToGIDMap")));
    let cid_to_gid = gid_bytes
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let w_at = cid_font.find("/W [").expect("/W") + 4;
    let w_txt = &cid_font[w_at..w_at + cid_font[w_at..].find("] /").unwrap() + 1];
    let toks: Vec<&str> = w_txt
        .split_whitespace()
        .map(|t| t.trim_matches(['[', ']']))
        .filter(|t| !t.is_empty())
        .collect();
    assert_eq!(toks.len() % 2, 0, "/W : paires « cid [largeur] »");
    let widths = toks
        .chunks(2)
        .map(|p| (p[0].parse().unwrap(), p[1].parse().unwrap()))
        .collect();
    let base_font = type0
        .split("/BaseFont /")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    PdfFont {
        type0,
        cid_font,
        descriptor,
        base_font,
        font_file,
        cid_to_gid,
        widths,
        to_unicode: to_unicode(pdf),
    }
}
