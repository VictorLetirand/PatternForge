//! Lecture indépendante des pages d'un PDF de rapport de contrôle (US-08.5) : texte placé (position,
//! corps, chaîne décodée par la table `ToUnicode` **du fichier**), filets, format de page.
#![allow(dead_code)]

use super::pdf_text::readable;

const PT_PER_MM: f64 = 72.0 / 25.4;

#[derive(Debug, Clone, PartialEq)]
pub struct TextOp {
    pub x_mm: f64,
    pub y_mm: f64,
    pub size_pt: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub x1_mm: f64,
    pub x2_mm: f64,
    pub y_mm: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    pub texts: Vec<TextOp>,
    pub rules: Vec<Rule>,
}

impl Page {
    /// Tout le texte de la page, une ligne placée par ligne, dans l'ordre du flux.
    pub fn joined(&self) -> String {
        self.texts
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Pages du PDF (flux de contenu `stream\n` … `endstream`).
pub fn pages(bytes: &[u8]) -> Vec<Page> {
    let text = readable(bytes);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").expect("endstream");
        out.push(read_stream(&body[..end]));
        rest = &body[end + 9..];
    }
    out
}

fn read_stream(stream: &str) -> Page {
    let mut page = Page::default();
    for line in stream.lines() {
        if let Some(rest) = line.strip_prefix("BT /F1 ") {
            // « {size} Tf 0 g {x} {y} Td ({texte}) Tj ET »
            let mut it = rest.split_whitespace();
            let size: f64 = it.next().unwrap().parse().unwrap();
            assert_eq!(it.next(), Some("Tf"));
            assert_eq!((it.next(), it.next()), (Some("0"), Some("g")));
            let x: f64 = it.next().unwrap().parse().unwrap();
            let y: f64 = it.next().unwrap().parse().unwrap();
            assert_eq!(it.next(), Some("Td"));
            let open = line.find(" Td (").expect("chaîne") + 5;
            let close = line.rfind(") Tj ET").expect("fin de chaîne");
            page.texts.push(TextOp {
                x_mm: x / PT_PER_MM,
                y_mm: y / PT_PER_MM,
                size_pt: size,
                text: unescape(&line[open..close]),
            });
        } else if line.ends_with(" l S") {
            // « x1 y1 m x2 y2 l S » : filet horizontal
            let t: Vec<&str> = line.split_whitespace().collect();
            assert!(t.len() == 7 && t[2] == "m" && t[5] == "l", "trait : {line}");
            let v: Vec<f64> = [t[0], t[1], t[3], t[4]]
                .iter()
                .map(|x| x.parse().unwrap())
                .collect();
            assert!((v[1] - v[3]).abs() < 1e-9, "filet horizontal");
            page.rules.push(Rule {
                x1_mm: v[0] / PT_PER_MM,
                x2_mm: v[2] / PT_PER_MM,
                y_mm: v[1] / PT_PER_MM,
            });
        }
    }
    page
}

/// Dimensions (mm) de chaque page d'après les `/MediaBox`.
pub fn media_boxes(bytes: &[u8]) -> Vec<(f64, f64)> {
    let text = std::str::from_utf8(bytes).expect("PDF ASCII");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("/MediaBox [0 0 ") {
        let tail = &rest[i + 15..];
        let end = tail.find(']').unwrap();
        let v: Vec<f64> = tail[..end]
            .split_whitespace()
            .map(|t| t.parse().unwrap())
            .collect();
        out.push((v[0] / PT_PER_MM, v[1] / PT_PER_MM));
        rest = &tail[end..];
    }
    out
}
