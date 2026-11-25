//! Police embarquée du PDF : lecture minimale d'un fichier TrueType (`head`, `hhea`, `maxp`,
//! `hmtx`, `loca`, `glyf`, `cmap`) et sous-ensemble de glyphes (US-06.6).
//!
//! La police est un sous-ensemble de **Noto Sans Regular** (licence SIL OFL 1.1, voir
//! `assets/fonts/`). Les largeurs de texte du PDF viennent de ses vraies largeurs d'avance,
//! jamais d'une estimation. Aucune dépendance, aucun accès réseau ; toute lecture est bornée
//! (un fichier malformé donne `None`, jamais un `panic`).
//!
//! Numéros de glyphes **conservés** dans le sous-ensemble (les glyphes inutilisés sont vides) :
//! pas de renumérotation, donc aucun glyphe composite à réécrire.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Octets du sous-ensemble Noto Sans Regular livré avec l'application.
static EMBEDDED_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular-subset.ttf");

type Tag = [u8; 4];

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        b.get(o..o.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn i16_at(b: &[u8], o: usize) -> Option<i16> {
    u16_at(b, o).map(|v| v as i16)
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(o..o.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Police TrueType lue (tables empruntées à l'octet source).
pub struct Face<'a> {
    tables: Vec<(Tag, &'a [u8])>,
    units_per_em: u16,
    num_glyphs: u16,
    num_hmetrics: u16,
    long_loca: bool,
    /// Caractère → glyphe (hors `.notdef`).
    cmap: BTreeMap<char, u16>,
    /// Caractère → CID du PDF : rang (à partir de 1) dans l'ordre des caractères de la police.
    cids: BTreeMap<char, u16>,
    /// CID − 1 → caractère.
    by_cid: Vec<char>,
}

impl<'a> Face<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Face<'a>> {
        let count = usize::from(u16_at(data, 4)?);
        let mut tables = Vec::with_capacity(count);
        for i in 0..count {
            let r = 12 + 16 * i;
            let tag: Tag = data.get(r..r + 4)?.try_into().ok()?;
            let off = u32_at(data, r + 8)? as usize;
            let len = u32_at(data, r + 12)? as usize;
            tables.push((tag, data.get(off..off.checked_add(len)?)?));
        }
        let find = |t: &[u8; 4]| tables.iter().find(|(g, _)| g == t).map(|(_, b)| *b);
        let head = find(b"head")?;
        let hhea = find(b"hhea")?;
        let maxp = find(b"maxp")?;
        let hmtx = find(b"hmtx")?;
        let loca = find(b"loca")?;
        find(b"glyf")?;
        let units_per_em = u16_at(head, 18)?;
        let long_loca = i16_at(head, 50)? != 0;
        let num_glyphs = u16_at(maxp, 4)?;
        let num_hmetrics = u16_at(hhea, 34)?;
        if units_per_em == 0
            || num_glyphs == 0
            || num_hmetrics == 0
            || num_hmetrics > num_glyphs
            || maxp.len() < 6
            || head.len() < 54
            || hhea.len() < 36
            || hmtx.len() < 4 * usize::from(num_hmetrics)
            || loca.len() < (usize::from(num_glyphs) + 1) * if long_loca { 4 } else { 2 }
        {
            return None;
        }
        let cmap = parse_cmap(find(b"cmap")?, num_glyphs)?;
        let by_cid: Vec<char> = cmap.keys().copied().collect();
        if by_cid.len() >= usize::from(u16::MAX) {
            return None;
        }
        let cids = by_cid
            .iter()
            .enumerate()
            .map(|(i, c)| (*c, i as u16 + 1))
            .collect();
        Some(Face {
            tables,
            units_per_em,
            num_glyphs,
            num_hmetrics,
            long_loca,
            cmap,
            cids,
            by_cid,
        })
    }

    pub fn table(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        self.tables.iter().find(|(g, _)| g == tag).map(|(_, b)| *b)
    }

    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Glyphe d'un caractère, `None` s'il est absent de la police.
    pub fn glyph(&self, ch: char) -> Option<u16> {
        self.cmap.get(&ch).copied()
    }

    /// Largeur d'avance (unités de police) d'un glyphe.
    pub fn advance(&self, gid: u16) -> u16 {
        let idx = gid.min(self.num_hmetrics - 1);
        self.table(b"hmtx")
            .and_then(|t| u16_at(t, 4 * usize::from(idx)))
            .unwrap_or(0)
    }

    fn left_bearing(&self, gid: u16) -> i16 {
        let Some(t) = self.table(b"hmtx") else {
            return 0;
        };
        let g = usize::from(gid);
        let n = usize::from(self.num_hmetrics);
        if g < n {
            i16_at(t, 4 * g + 2)
        } else {
            i16_at(t, 4 * n + 2 * (g - n))
        }
        .unwrap_or(0)
    }

    /// Octets du glyphe `gid` (vide pour un glyphe sans contour).
    fn glyph_data(&self, gid: u16) -> &'a [u8] {
        if gid >= self.num_glyphs {
            return &[];
        }
        let loca = self.table(b"loca").unwrap_or(&[]);
        let g = usize::from(gid);
        let (a, b) = if self.long_loca {
            (u32_at(loca, 4 * g), u32_at(loca, 4 * g + 4))
        } else {
            (
                u16_at(loca, 2 * g).map(|v| u32::from(v) * 2),
                u16_at(loca, 2 * g + 2).map(|v| u32::from(v) * 2),
            )
        };
        match (a, b) {
            (Some(a), Some(b)) if b > a => self
                .table(b"glyf")
                .and_then(|t| t.get(a as usize..b as usize))
                .unwrap_or(&[]),
            _ => &[],
        }
    }

    /// Glyphes référencés par un glyphe composite (vide pour un glyphe simple).
    fn components(&self, gid: u16) -> Vec<u16> {
        let d = self.glyph_data(gid);
        let mut out = Vec::new();
        if d.len() < 10 || i16_at(d, 0).is_none_or(|n| n >= 0) {
            return out;
        }
        let mut o = 10;
        while let (Some(flags), Some(g)) = (u16_at(d, o), u16_at(d, o + 2)) {
            out.push(g);
            o += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
            if flags & 0x0008 != 0 {
                o += 2;
            } else if flags & 0x0040 != 0 {
                o += 4;
            } else if flags & 0x0080 != 0 {
                o += 8;
            }
            if flags & 0x0020 == 0 {
                break;
            }
        }
        out
    }

    /// Tables d'un sous-ensemble contenant `gids` (+ `.notdef` + composants), numéros conservés.
    fn subset_tables(&self, gids: &BTreeSet<u16>) -> BTreeMap<Tag, Vec<u8>> {
        let mut keep: BTreeSet<u16> = gids
            .iter()
            .copied()
            .filter(|g| *g < self.num_glyphs)
            .collect();
        keep.insert(0);
        let mut stack: Vec<u16> = keep.iter().copied().collect();
        while let Some(g) = stack.pop() {
            for c in self.components(g) {
                if c < self.num_glyphs && keep.insert(c) {
                    stack.push(c);
                }
            }
        }
        let out_n = keep.last().copied().unwrap_or(0) + 1;
        let (mut glyf, mut loca, mut hmtx) = (Vec::new(), Vec::new(), Vec::new());
        for g in 0..out_n {
            loca.extend((glyf.len() as u32).to_be_bytes());
            if keep.contains(&g) {
                glyf.extend_from_slice(self.glyph_data(g));
                while glyf.len() % 4 != 0 {
                    glyf.push(0);
                }
                hmtx.extend(self.advance(g).to_be_bytes());
                hmtx.extend(self.left_bearing(g).to_be_bytes());
            } else {
                hmtx.extend([0, 0, 0, 0]);
            }
        }
        loca.extend((glyf.len() as u32).to_be_bytes());
        let patched = |tag: &[u8; 4], at: usize, v: &[u8]| -> Vec<u8> {
            let mut t = self.table(tag).unwrap_or(&[]).to_vec();
            t[at..at + v.len()].copy_from_slice(v);
            t
        };
        let mut tables = BTreeMap::new();
        tables.insert(*b"head", patched(b"head", 50, &1i16.to_be_bytes()));
        tables.insert(*b"hhea", patched(b"hhea", 34, &out_n.to_be_bytes()));
        tables.insert(*b"maxp", patched(b"maxp", 4, &out_n.to_be_bytes()));
        tables.insert(*b"hmtx", hmtx);
        tables.insert(*b"loca", loca);
        tables.insert(*b"glyf", glyf);
        // Programmes de hinting : les glyphes copiés peuvent les appeler.
        for tag in [b"cvt ", b"fpgm", b"prep"] {
            if let Some(t) = self.table(tag) {
                tables.insert(*tag, t.to_vec());
            }
        }
        tables
    }

    /// Police TrueType minimale (pour `FontFile2` d'un PDF) ne contenant que `gids`.
    pub fn subset(&self, gids: &BTreeSet<u16>) -> Vec<u8> {
        build_font(self.subset_tables(gids))
    }

    /// Sous-ensemble autonome (avec `cmap`, `name`, `OS/2`, `post`) pour les caractères donnés ;
    /// sert à produire le fichier de police livré dans le dépôt (voir `assets/fonts/README.md`).
    pub fn standalone_subset(&self, chars: &BTreeSet<char>) -> Vec<u8> {
        let map: BTreeMap<char, u16> = chars
            .iter()
            .filter_map(|c| self.glyph(*c).map(|g| (*c, g)))
            .collect();
        let gids: BTreeSet<u16> = map.values().copied().collect();
        let mut tables = self.subset_tables(&gids);
        for tag in [b"name", b"OS/2"] {
            if let Some(t) = self.table(tag) {
                tables.insert(*tag, t.to_vec());
            }
        }
        tables.insert(*b"cmap", build_cmap12(&map));
        let mut post = vec![0u8; 32];
        post[..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
        tables.insert(*b"post", post);
        build_font(tables)
    }

    // --- CID du PDF -------------------------------------------------------------------------

    /// CID d'un caractère ; un caractère absent de la police devient « ? » (documenté), et le
    /// CID 0 (`.notdef`) si « ? » manquait aussi.
    pub fn cid(&self, ch: char) -> u16 {
        self.cids
            .get(&ch)
            .or_else(|| self.cids.get(&'?'))
            .copied()
            .unwrap_or(0)
    }

    /// Caractère d'un CID (`None` pour 0 ou hors table).
    pub fn char_of_cid(&self, cid: u16) -> Option<char> {
        usize::from(cid)
            .checked_sub(1)
            .and_then(|i| self.by_cid.get(i))
            .copied()
    }

    /// Glyphe d'un CID (0 = `.notdef`).
    pub fn gid_of_cid(&self, cid: u16) -> u16 {
        self.char_of_cid(cid)
            .and_then(|c| self.glyph(c))
            .unwrap_or(0)
    }

    /// Largeur d'avance (unités de police) d'un CID.
    pub fn advance_of_cid(&self, cid: u16) -> u16 {
        self.advance(self.gid_of_cid(cid))
    }

    /// Largeur (unités de police) d'un texte tel qu'il est écrit dans le PDF.
    pub fn text_units(&self, text: &str) -> u64 {
        text.chars()
            .map(|c| u64::from(self.advance_of_cid(self.cid(c))))
            .sum()
    }

    /// Plus grand CID attribué.
    pub fn max_cid(&self) -> u16 {
        self.by_cid.len() as u16
    }
}

/// Police livrée avec l'application.
pub fn embedded() -> &'static Face<'static> {
    static FACE: OnceLock<Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| Face::parse(EMBEDDED_BYTES).expect("police embarquée valide"))
}

/// Largeur (mm) d'un texte de corps `size_pt`, d'après les largeurs d'avance de la police
/// embarquée (celles que le PDF utilise pour l'afficher).
pub fn text_width_mm(text: &str, size_pt: f64) -> f64 {
    let f = embedded();
    f.text_units(text) as f64 / f64::from(f.units_per_em()) * size_pt * 25.4 / 72.0
}

fn parse_cmap(t: &[u8], num_glyphs: u16) -> Option<BTreeMap<char, u16>> {
    let n = usize::from(u16_at(t, 2)?);
    let mut subs: Vec<(u16, usize)> = Vec::new();
    for i in 0..n {
        let (platform, off) = (u16_at(t, 4 + 8 * i)?, u32_at(t, 8 + 8 * i)? as usize);
        if platform == 0 || platform == 3 {
            subs.push((u16_at(t, off)?, off));
        }
    }
    let mut map = BTreeMap::new();
    // Format 12 d'abord (il prime), puis format 4 pour compléter.
    for fmt in [12u16, 4] {
        for (_, off) in subs.iter().filter(|(f, _)| *f == fmt) {
            let sub = t.get(*off..)?;
            if fmt == 12 {
                for g in 0..u32_at(sub, 12)? as usize {
                    let o = 16 + 12 * g;
                    let (a, b, gid) = (u32_at(sub, o)?, u32_at(sub, o + 4)?, u32_at(sub, o + 8)?);
                    if b < a || b - a > 0xFFFF {
                        return None; // groupe aberrant : police refusée
                    }
                    for c in a..=b.min(0x10_FFFF) {
                        let id = gid.checked_add(c - a)?;
                        if id != 0 && id < u32::from(num_glyphs) {
                            if let Some(ch) = char::from_u32(c) {
                                map.entry(ch).or_insert(id as u16);
                            }
                        }
                    }
                }
            } else {
                let segx2 = usize::from(u16_at(sub, 6)?);
                let (end_o, start_o) = (14, 16 + segx2);
                let (delta_o, range_o) = (start_o + segx2, start_o + 2 * segx2);
                for s in 0..segx2 / 2 {
                    let end = u16_at(sub, end_o + 2 * s)?;
                    let start = u16_at(sub, start_o + 2 * s)?;
                    let delta = u16_at(sub, delta_o + 2 * s)?;
                    let ro = u16_at(sub, range_o + 2 * s)?;
                    if start > end || start == 0xFFFF {
                        continue;
                    }
                    for c in start..=end {
                        let gid = if ro == 0 {
                            c.wrapping_add(delta)
                        } else {
                            let at = range_o + 2 * s + usize::from(ro) + 2 * usize::from(c - start);
                            match u16_at(sub, at)? {
                                0 => 0,
                                g => g.wrapping_add(delta),
                            }
                        };
                        if gid != 0 && gid < num_glyphs {
                            if let Some(ch) = char::from_u32(u32::from(c)) {
                                map.entry(ch).or_insert(gid);
                            }
                        }
                    }
                }
            }
        }
    }
    Some(map)
}

/// `cmap` minimal (plate-forme 3, encodage 10, format 12).
fn build_cmap12(map: &BTreeMap<char, u16>) -> Vec<u8> {
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for (c, g) in map {
        let (c, g) = (*c as u32, u32::from(*g));
        match groups.last_mut() {
            Some((start, end, gid)) if *end + 1 == c && *gid + (c - *start) == g => *end = c,
            _ => groups.push((c, c, g)),
        }
    }
    let len = 16 + 12 * groups.len();
    let mut t = Vec::with_capacity(12 + len);
    t.extend([0, 0, 0, 1, 0, 3, 0, 10]);
    t.extend(12u32.to_be_bytes());
    t.extend(12u16.to_be_bytes());
    t.extend(0u16.to_be_bytes());
    t.extend((len as u32).to_be_bytes());
    t.extend(0u32.to_be_bytes());
    t.extend((groups.len() as u32).to_be_bytes());
    for (a, b, g) in groups {
        t.extend(a.to_be_bytes());
        t.extend(b.to_be_bytes());
        t.extend(g.to_be_bytes());
    }
    t
}

fn checksum(b: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in b.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(w));
    }
    sum
}

/// Assemble un fichier TrueType : répertoire trié, tables alignées sur 4 octets, sommes de
/// contrôle et `checkSumAdjustment` de `head` calculés.
fn build_font(mut tables: BTreeMap<Tag, Vec<u8>>) -> Vec<u8> {
    if let Some(head) = tables.get_mut(b"head") {
        head[8..12].copy_from_slice(&[0; 4]);
    }
    let n = tables.len();
    let mut entry_selector = 0u16;
    while 1usize << (entry_selector + 1) <= n {
        entry_selector += 1;
    }
    let search_range = 16u16 << entry_selector;
    let mut out = Vec::new();
    out.extend(0x0001_0000u32.to_be_bytes());
    out.extend((n as u16).to_be_bytes());
    out.extend(search_range.to_be_bytes());
    out.extend(entry_selector.to_be_bytes());
    out.extend((n as u16 * 16 - search_range).to_be_bytes());
    let mut offset = 12 + 16 * n;
    let mut head_at = None;
    for (tag, data) in &tables {
        out.extend(tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend((offset as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = Some(offset);
        }
        offset += data.len().div_ceil(4) * 4;
    }
    for data in tables.values() {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    if let Some(at) = head_at {
        let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[at + 8..at + 12].copy_from_slice(&adj.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_police_embarquee_est_lue_avec_les_largeurs_attendues() {
        let f = embedded();
        assert_eq!(f.units_per_em(), 1000);
        // Largeurs d'avance de Noto Sans Regular (valeurs écrites à la main) : chiffre 572,
        // espace 260, « m » 935, « ? » 434.
        let w = |c: char| f.advance(f.glyph(c).unwrap());
        assert_eq!((w('0'), w(' '), w('m'), w('?')), (572, 260, 935, 434));
        assert_eq!(f.text_units("0 m"), 572 + 260 + 935);
        assert_eq!(f.text_units(""), 0);
        // Absent de la police (cyrillique) : compté comme « ? ».
        assert_eq!(f.text_units("Ж"), 434);
    }

    #[test]
    fn cid_stables_distincts_et_remplacement_par_point_d_interrogation() {
        let f = embedded();
        let (e, q) = (f.cid('é'), f.cid('?'));
        assert!(e != 0 && q != 0 && e != q);
        assert_eq!(f.cid('日'), q);
        assert_eq!(f.char_of_cid(e), Some('é'));
        assert_eq!(f.char_of_cid(0), None);
        assert_eq!(f.char_of_cid(f.max_cid() + 1), None);
        // Espace insécable et espace : deux CID, donc deux textes distincts au `ToUnicode`.
        assert_ne!(f.cid('\u{a0}'), f.cid(' '));
    }

    #[test]
    fn sous_ensemble_autonome_relu_et_somme_de_controle_de_la_police() {
        let f = embedded();
        let chars: BTreeSet<char> = ['A', 'é', ' '].into_iter().collect();
        let bytes = f.standalone_subset(&chars);
        let s = Face::parse(&bytes).expect("sous-ensemble lisible");
        assert!(s.glyph('A').is_some() && s.glyph('é').is_some());
        assert_eq!(s.glyph('B'), None);
        assert_eq!(
            s.advance(s.glyph('A').unwrap()),
            f.advance(f.glyph('A').unwrap())
        );
        // Somme de contrôle de l'ensemble du fichier (propriété de `checkSumAdjustment`).
        assert_eq!(checksum(&bytes), 0xB1B0_AFBA);
        assert_eq!(checksum(EMBEDDED_BYTES), 0xB1B0_AFBA);
        // `é` est un glyphe composite : son composant (la lettre « e ») est embarqué aussi.
        let e = f.glyph('é').unwrap();
        assert!(!f.components(e).is_empty());
        for c in f.components(e) {
            assert!(!s.glyph_data(c).is_empty(), "composant {c} absent");
        }
    }

    #[test]
    fn polices_aberrantes_refusees_ou_lues_sans_panic() {
        assert!(Face::parse(&[]).is_none());
        assert!(Face::parse(&[0; 11]).is_none());
        // Troncatures de la police embarquée.
        for cut in (0..EMBEDDED_BYTES.len()).step_by(1999) {
            let _ = Face::parse(&EMBEDDED_BYTES[..cut]);
        }
        // Octets altérés (graine fixe) : jamais de panic, quel que soit le résultat.
        let mut seed = 0x1234_5678_9ABC_DEF1u64;
        for _ in 0..300 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let mut data = EMBEDDED_BYTES.to_vec();
            for k in 0..4 {
                let at = ((seed >> (8 * k)) as usize ^ (seed as usize >> 3)) % data.len();
                data[at] ^= (seed >> (16 + k)) as u8 | 1;
            }
            if let Some(face) = Face::parse(&data) {
                let gids: BTreeSet<u16> = [0, 3, 171, 400, 65535].into_iter().collect();
                let _ = face.subset(&gids);
                let _ = face.text_units("Écharpe à pli — Ж");
                let _ = face.cid('é');
            }
        }
    }
}
