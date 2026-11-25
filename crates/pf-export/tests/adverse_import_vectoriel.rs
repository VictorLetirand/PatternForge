//! US-12.4 - Campagne adverse de l'import SVG / PDF : 260 fichiers a graine fixe, generes
//! aleatoirement puis corrompus. Oracle **ecrit ici, independamment du lecteur** : chaque fichier
//! valide est fabrique avec sa longueur totale calculee par le generateur (segments, Beziers par
//! echantillonnage) ; chaque corruption a une issue attendue connue (refus, ou element ignore ET
//! liste), jamais un import partiel muet. Aucun panic, quelle que soit l'entree.

use pf_core::{ImportItem, MAX_IMPORT_OBJECTS};
use pf_export::pdf_import::PdfDocument;
use pf_export::svg_import::{SvgDocument, SvgUnitChoice};
use pf_export::vector_import::{IgnoreReason, VectorBuild, VectorImportError};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// Nombre a deux decimales dans [-lim ; lim], tel qu'ecrit dans le fichier.
    fn num(&mut self, lim: i64) -> f64 {
        (self.below(2 * lim as u64 * 100 + 1) as i64 - lim * 100) as f64 / 100.0
    }
}

fn len2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn bez_len(p: [(f64, f64); 4]) -> f64 {
    let at = |t: f64| {
        let u = 1.0 - t;
        let f = |i: usize| {
            let get = |k: usize| if i == 0 { p[k].0 } else { p[k].1 };
            u * u * u * get(0)
                + 3.0 * u * u * t * get(1)
                + 3.0 * u * t * t * get(2)
                + t * t * t * get(3)
        };
        (f(0), f(1))
    };
    (0..3000)
        .map(|k| len2(at(k as f64 / 3000.0), at((k + 1) as f64 / 3000.0)))
        .sum()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * (1.0 + b.abs())
}

fn item_len(i: &ImportItem) -> f64 {
    match i {
        ImportItem::Segment { from, to } => len2((from.x, from.y), (to.x, to.y)),
        ImportItem::Circle { radius_mm, .. } => std::f64::consts::TAU * radius_mm,
        ImportItem::Bezier { start, c1, c2, end } => bez_len([
            (start.x, start.y),
            (c1.x, c1.y),
            (c2.x, c2.y),
            (end.x, end.y),
        ]),
        other => panic!("{other:?}"),
    }
}

fn total(b: &VectorBuild) -> f64 {
    b.objects.iter().map(|o| item_len(&o.item)).sum()
}

/// Invariants de tout import reussi, quelle que soit l'entree.
fn check_sane(b: &VectorBuild) {
    assert_eq!(
        b.report.read,
        b.report.imported_total() + b.report.ignored_total(),
        "tout element lu est importe ou liste"
    );
    assert_eq!(b.report.objects, b.objects.len());
    assert!(b.objects.len() <= MAX_IMPORT_OBJECTS);
    let ok = |p: &pf_geometry::PointMm| {
        p.x.is_finite() && p.y.is_finite() && p.x.abs() <= 1e6 && p.y.abs() <= 1e6
    };
    for o in &b.objects {
        match &o.item {
            ImportItem::Segment { from, to } => assert!(ok(from) && ok(to)),
            ImportItem::Bezier { start, c1, c2, end } => {
                assert!(ok(start) && ok(c1) && ok(c2) && ok(end))
            }
            ImportItem::Circle { center, radius_mm } => {
                assert!(ok(center) && radius_mm.is_finite() && *radius_mm > 0.0)
            }
            ImportItem::Arc {
                center, start, end, ..
            } => assert!(ok(center) && ok(start) && ok(end)),
            other => panic!("{other:?}"),
        }
    }
    if let Some(bb) = b.bbox_mm {
        assert!(bb.min.x.is_finite() && bb.max.y.is_finite() && bb.width() >= 0.0);
    } else {
        assert!(b.objects.is_empty());
    }
}

// ------------------------------------------------------------------------------ SVG genere

struct SvgElem {
    xml: String,
    len_mm: f64,
}

fn fmt(v: f64) -> String {
    format!("{v:.2}")
}

fn svg_element(r: &mut Rng) -> SvgElem {
    match r.below(7) {
        0 => {
            let (x, y, w, h) = (
                r.num(50),
                r.num(50),
                r.num(30).abs() + 1.0,
                r.num(30).abs() + 1.0,
            );
            SvgElem {
                xml: format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                    fmt(x),
                    fmt(y),
                    fmt(w),
                    fmt(h)
                ),
                len_mm: 2.0 * (w + h),
            }
        }
        1 => {
            let p = [(r.num(50), r.num(50)), (r.num(50), r.num(50))];
            SvgElem {
                xml: format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                    fmt(p[0].0),
                    fmt(p[0].1),
                    fmt(p[1].0),
                    fmt(p[1].1)
                ),
                len_mm: len2(p[0], p[1]),
            }
        }
        2 | 3 => {
            let n = 2 + r.below(6) as usize;
            let pts: Vec<(f64, f64)> = (0..n).map(|_| (r.num(50), r.num(50))).collect();
            let list: Vec<String> = pts
                .iter()
                .map(|p| format!("{},{}", fmt(p.0), fmt(p.1)))
                .collect();
            let closed = r.below(2) == 0;
            let mut l: f64 = pts.windows(2).map(|w| len2(w[0], w[1])).sum();
            if closed {
                l += len2(pts[n - 1], pts[0]);
            }
            SvgElem {
                xml: format!(
                    "<{} points=\"{}\"/>",
                    if closed { "polygon" } else { "polyline" },
                    list.join(" ")
                ),
                len_mm: l,
            }
        }
        4 => {
            let (cx, cy, rad) = (r.num(50), r.num(50), r.num(20).abs() + 1.0);
            SvgElem {
                xml: format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                    fmt(cx),
                    fmt(cy),
                    fmt(rad)
                ),
                len_mm: std::f64::consts::TAU * rad,
            }
        }
        5 => {
            // Chemin M, puis L / H / V / C absolus ou relatifs ; longueurs calculees ici.
            let mut cur = (r.num(40), r.num(40));
            let mut d = format!("M {} {}", fmt(cur.0), fmt(cur.1));
            let mut l = 0.0;
            for _ in 0..(1 + r.below(5)) {
                let rel = r.below(2) == 0;
                match r.below(4) {
                    0 => {
                        let p = (r.num(40), r.num(40));
                        let to = if rel { (cur.0 + p.0, cur.1 + p.1) } else { p };
                        d += &format!(" {} {} {}", if rel { "l" } else { "L" }, fmt(p.0), fmt(p.1));
                        l += len2(cur, to);
                        cur = to;
                    }
                    1 => {
                        let x = r.num(40);
                        let to = (if rel { cur.0 + x } else { x }, cur.1);
                        d += &format!(" {} {}", if rel { "h" } else { "H" }, fmt(x));
                        l += len2(cur, to);
                        cur = to;
                    }
                    2 => {
                        let y = r.num(40);
                        let to = (cur.0, if rel { cur.1 + y } else { y });
                        d += &format!(" {} {}", if rel { "v" } else { "V" }, fmt(y));
                        l += len2(cur, to);
                        cur = to;
                    }
                    _ => {
                        let q: Vec<(f64, f64)> = (0..3).map(|_| (r.num(40), r.num(40))).collect();
                        let abs: Vec<(f64, f64)> = q
                            .iter()
                            .map(|p| if rel { (cur.0 + p.0, cur.1 + p.1) } else { *p })
                            .collect();
                        d += &format!(
                            " {} {} {} {} {} {} {}",
                            if rel { "c" } else { "C" },
                            fmt(q[0].0),
                            fmt(q[0].1),
                            fmt(q[1].0),
                            fmt(q[1].1),
                            fmt(q[2].0),
                            fmt(q[2].1)
                        );
                        l += bez_len([cur, abs[0], abs[1], abs[2]]);
                        cur = abs[2];
                    }
                }
            }
            SvgElem {
                xml: format!("<path d=\"{d}\"/>"),
                len_mm: l,
            }
        }
        _ => {
            // Chemin ferme par Z (un rectangle par m / h / v / z, relatif).
            let (x, y, w, h) = (
                r.num(40),
                r.num(40),
                r.num(20).abs() + 1.0,
                r.num(20).abs() + 1.0,
            );
            SvgElem {
                xml: format!(
                    "<path d=\"m{} {}h{}v{}h{}z\"/>",
                    fmt(x),
                    fmt(y),
                    fmt(w),
                    fmt(h),
                    fmt(-w)
                ),
                len_mm: 2.0 * (w + h),
            }
        }
    }
}

struct SvgFile {
    elems: Vec<SvgElem>,
    /// Chaque element est ecrit dans un groupe `translate scale` de facteur uniforme.
    scales: Vec<f64>,
}

impl SvgFile {
    fn text(&self, wrap: &dyn Fn(usize) -> Option<String>) -> String {
        let mut s = String::from(
            "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200mm\" height=\"200mm\" viewBox=\"0 0 200 200\">\n  <title>t</title>\n",
        );
        for (i, e) in self.elems.iter().enumerate() {
            let tf = wrap(i)
                .unwrap_or_else(|| format!("translate(100 100) scale({})", fmt(self.scales[i])));
            s += &format!("  <g transform=\"{tf}\">{}</g>\n", e.xml);
        }
        s += "</svg>\n";
        s
    }
    fn expected(&self) -> f64 {
        self.elems
            .iter()
            .zip(&self.scales)
            .map(|(e, s)| e.len_mm * s)
            .sum()
    }
}

fn svg_file(r: &mut Rng) -> SvgFile {
    let n = 1 + r.below(8) as usize;
    SvgFile {
        elems: (0..n).map(|_| svg_element(r)).collect(),
        scales: (0..n).map(|_| (1 + r.below(300)) as f64 / 100.0).collect(),
    }
}

fn import_svg(text: &str) -> Result<VectorBuild, VectorImportError> {
    SvgDocument::parse(text.as_bytes())?.build_choice(SvgUnitChoice::Declared, 1.0)
}

// ------------------------------------------------------------------------------ PDF genere

fn pdf_of(content: &[u8], stream_dict: &str, trailer: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    out.extend(
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>\nendobj\n",
    );
    out.extend(
        format!(
            "4 0 obj\n<< /Length {} {stream_dict} >>\nstream\n",
            content.len()
        )
        .into_bytes(),
    );
    out.extend(content);
    out.extend(b"\nendstream\nendobj\n");
    out.extend(format!("trailer\n<< /Size 5 /Root 1 0 R {trailer} >>\n%%EOF\n").into_bytes());
    out
}

const MM: f64 = 25.4 / 72.0;

/// Contenu : chemins `m l c re` sous un `cm` uniforme ; renvoie (flux, longueur en mm, operations).
fn pdf_content(r: &mut Rng) -> (String, f64, usize) {
    let s = (1 + r.below(300)) as f64 / 100.0;
    let (tx, ty) = (r.num(100), r.num(100));
    let mut c = format!("q {} 0 0 {} {} {} cm\n", fmt(s), fmt(s), fmt(tx), fmt(ty));
    let mut l = 0.0;
    let mut ops = 0;
    for _ in 0..(1 + r.below(6)) {
        match r.below(3) {
            0 => {
                let (a, b) = ((r.num(100), r.num(100)), (r.num(100), r.num(100)));
                c += &format!(
                    "{} {} m {} {} l S\n",
                    fmt(a.0),
                    fmt(a.1),
                    fmt(b.0),
                    fmt(b.1)
                );
                l += len2(a, b);
                ops += 3;
            }
            1 => {
                let p: Vec<(f64, f64)> = (0..4).map(|_| (r.num(100), r.num(100))).collect();
                c += &format!(
                    "{} {} m {} {} {} {} {} {} c S\n",
                    fmt(p[0].0),
                    fmt(p[0].1),
                    fmt(p[1].0),
                    fmt(p[1].1),
                    fmt(p[2].0),
                    fmt(p[2].1),
                    fmt(p[3].0),
                    fmt(p[3].1)
                );
                l += bez_len([p[0], p[1], p[2], p[3]]);
                ops += 2;
            }
            _ => {
                let (x, y, w, h) = (
                    r.num(50),
                    r.num(50),
                    r.num(40).abs() + 1.0,
                    r.num(40).abs() + 1.0,
                );
                c += &format!("{} {} {} {} re f\n", fmt(x), fmt(y), fmt(w), fmt(h));
                l += 2.0 * (w + h);
                ops += 5;
            }
        }
    }
    c += "Q\n";
    (c, l * s * MM, ops)
}

// ------------------------------------------------------------------------------ campagne

#[test]
fn campagne_adverse_import_vectoriel_260_fichiers_graine_fixe() {
    let mut r = Rng(0x5EED_0124_C0DE_F00D);
    let (mut valid_svg, mut valid_pdf, mut cut_svg, mut cut_pdf) = (0, 0, 0, 0);
    let (mut nan, mut degenerate, mut garbage, mut flate_bad, mut operand, mut deep) =
        (0, 0, 0, 0, 0, 0);
    let (mut refused, mut listed) = (0usize, 0usize);

    // 1. SVG valides (60) : longueur totale exacte, aucun ignore.
    for _ in 0..60 {
        let f = svg_file(&mut r);
        let b = import_svg(&f.text(&|_| None)).unwrap();
        check_sane(&b);
        assert!(
            near(total(&b), f.expected()),
            "{} contre {}",
            total(&b),
            f.expected()
        );
        assert_eq!(b.report.ignored_total(), 0);
        valid_svg += 1;
    }

    // 2. PDF valides (40), la moitie en Flate : longueur totale exacte.
    for k in 0..40 {
        let (c, want, _) = pdf_content(&mut r);
        let bytes = if k % 2 == 0 {
            pdf_of(c.as_bytes(), "", "")
        } else {
            pdf_of(
                &miniz_oxide::deflate::compress_to_vec_zlib(c.as_bytes(), 6),
                "/Filter /FlateDecode",
                "",
            )
        };
        let b = PdfDocument::parse(&bytes).unwrap().build(1, 1.0).unwrap();
        check_sane(&b);
        assert!(near(total(&b), want), "{} contre {want}", total(&b));
        assert_eq!(b.report.ignored_total(), 0);
        valid_pdf += 1;
    }

    // 3. SVG tronques (30) : couper avant `</svg>` = refus, jamais un import partiel.
    for _ in 0..30 {
        let f = svg_file(&mut r);
        let text = f.text(&|_| None);
        let end = text.rfind("</svg>").unwrap();
        let cut = r.below(end as u64) as usize;
        let res = import_svg(&text[..cut]);
        assert!(res.is_err(), "coupe a {cut}/{end} acceptee");
        refused += 1;
        cut_svg += 1;
    }

    // 4. SVG a nombre NaN (20) : l'element est ignore ET liste, les autres gardent leur longueur.
    for _ in 0..20 {
        let mut f = svg_file(&mut r);
        let k = r.below(f.elems.len() as u64) as usize;
        let bad = [
            "<path d=\"M NaN 0 L 5 5\"/>",
            "<path d=\"M 0 0 L inf 5\"/>",
            "<rect width=\"NaN\" height=\"3\"/>",
            "<circle r=\"-\"/>",
        ][r.below(4) as usize];
        f.elems[k] = SvgElem {
            xml: bad.into(),
            len_mm: 0.0,
        };
        let b = import_svg(&f.text(&|_| None)).unwrap();
        check_sane(&b);
        assert!(near(total(&b), f.expected()));
        assert_eq!(b.report.ignored_total(), 1);
        assert_eq!(b.report.ignored[0].reason, IgnoreReason::Invalid);
        listed += 1;
        nan += 1;
    }

    // 5. Transformations degenerees (20) : element ignore (Degenerate) et liste.
    for _ in 0..20 {
        let f = svg_file(&mut r);
        let k = r.below(f.elems.len() as u64) as usize;
        let tf = [
            "scale(0)",
            "matrix(1 2 2 4 0 0)",
            "scale(0 5)",
            "matrix(0 0 0 0 7 7)",
        ][r.below(4) as usize];
        let text = f.text(&|i| (i == k).then(|| tf.to_string()));
        let b = import_svg(&text).unwrap();
        check_sane(&b);
        let want = f.expected() - f.elems[k].len_mm * f.scales[k];
        assert!(near(total(&b), want), "{} contre {want}", total(&b));
        assert!(b
            .report
            .ignored
            .iter()
            .any(|i| i.reason == IgnoreReason::Degenerate));
        listed += 1;
        degenerate += 1;
    }

    // 6. SVG a imbrication absurde (10) : refus propre, pas de debordement de pile.
    for k in 0..10 {
        let depth = 600 + k * 7000;
        let text = format!(
            "<svg>{}<line x2=\"1\"/>{}</svg>",
            "<g>".repeat(depth),
            "</g>".repeat(depth)
        );
        assert!(SvgDocument::parse(text.as_bytes()).is_err());
        refused += 1;
        deep += 1;
    }

    // 7. PDF tronques (20) : couper avant `trailer` = refus.
    for _ in 0..20 {
        let (c, _, _) = pdf_content(&mut r);
        let bytes = pdf_of(c.as_bytes(), "", "");
        let t = bytes.windows(7).position(|w| w == b"trailer").unwrap();
        let cut = r.below(t as u64) as usize;
        let res = PdfDocument::parse(&bytes[..cut]);
        assert!(res.is_err(), "coupe a {cut}/{t} acceptee");
        refused += 1;
        cut_pdf += 1;
    }

    // 8. PDF a operande retiree ou remplacee par NaN (20) : refus de structure.
    for k in 0..20 {
        let (c, _, _) = pdf_content(&mut r);
        let toks: Vec<&str> = c.split_whitespace().collect();
        // Une operande numerique apres le `cm` initial (indice >= 8).
        let candidates: Vec<usize> = (8..toks.len())
            .filter(|i| toks[*i].parse::<f64>().is_ok())
            .collect();
        let i = candidates[r.below(candidates.len() as u64) as usize];
        let mut t2: Vec<String> = toks.iter().map(|s| s.to_string()).collect();
        if k % 2 == 0 {
            t2.remove(i);
        } else {
            t2[i] = "NaN".into();
        }
        let bytes = pdf_of(t2.join(" ").as_bytes(), "", "");
        let res = PdfDocument::parse(&bytes).and_then(|d| d.build(1, 1.0));
        assert!(
            matches!(res, Err(VectorImportError::Structure(_))),
            "{res:?}"
        );
        refused += 1;
        operand += 1;
    }

    // 9. Flux Flate corrompu (10) : refus (somme de controle ou structure).
    for _ in 0..10 {
        let (c, _, _) = pdf_content(&mut r);
        let mut z = miniz_oxide::deflate::compress_to_vec_zlib(c.as_bytes(), 6);
        let i = 2 + r.below((z.len() - 2) as u64) as usize;
        z[i] ^= 1 << r.below(8);
        let bytes = pdf_of(&z, "/Filter /FlateDecode", "");
        let res = PdfDocument::parse(&bytes);
        assert!(res.is_err(), "flux corrompu accepte");
        refused += 1;
        flate_bad += 1;
    }

    // 10. Octets quelconques (30) : jamais de panic, refus ou rapport coherent.
    for k in 0..30 {
        let n = 1 + r.below(400) as usize;
        let mut bytes: Vec<u8> = (0..n).map(|_| r.below(256) as u8).collect();
        if k % 3 == 1 {
            let mut p = b"<svg>".to_vec();
            p.extend(&bytes);
            bytes = p;
        } else if k % 3 == 2 {
            let mut p = b"%PDF-1.4\n".to_vec();
            p.extend(&bytes);
            bytes = p;
        }
        if let Ok(d) = SvgDocument::parse(&bytes) {
            if let Ok(b) = d.build_choice(SvgUnitChoice::Declared, 1.0) {
                check_sane(&b);
            }
        }
        if let Ok(d) = PdfDocument::parse(&bytes) {
            for p in 1..=d.page_count().min(3) {
                if let Ok(b) = d.build(p, 1.0) {
                    check_sane(&b);
                }
            }
        }
        garbage += 1;
    }

    let n = valid_svg
        + valid_pdf
        + cut_svg
        + cut_pdf
        + nan
        + degenerate
        + garbage
        + flate_bad
        + operand
        + deep;
    assert!(n >= 150, "{n} sequences");
    assert_eq!(n, 260);
    // Compteurs de couverture : chaque famille a bien tourne.
    assert_eq!((valid_svg, valid_pdf, cut_svg, cut_pdf), (60, 40, 30, 20));
    assert_eq!(
        (nan, degenerate, garbage, flate_bad, operand, deep),
        (20, 20, 30, 10, 20, 10)
    );
    assert_eq!(refused, 30 + 10 + 20 + 20 + 10);
    assert_eq!(listed, 40);
}
