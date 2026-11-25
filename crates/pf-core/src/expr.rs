//! US-03.1a — évaluateur d'expressions arithmétiques (module **pur**, sans projet).
//!
//! Grammaire (priorité croissante, opérateurs binaires associatifs à **gauche**) :
//!
//! ```text
//! expr    = terme { ("+" | "-") terme }
//! terme   = unaire { ("*" | "/") unaire }
//! unaire  = "-" unaire | primaire
//! primaire= nombre | "pi" | nom | fonction "(" expr { ";" expr } ")" | "(" expr ")"
//! ```
//!
//! - Nombres décimaux : `12`, `12.5`, `12,5`, `.5` ; **la virgule est une décimale** (saisie
//!   française) et **le séparateur d'arguments est `;`** : `min(1,5;2)` vaut `min(1.5 ; 2)`, et
//!   `min(1,2)` (un seul argument `1,2`) est refusé. Pas de notation scientifique (`1e3` refusé),
//!   pas de séparateur de milliers, pas de `+` unaire, pas de puissance.
//! - Fonctions (en minuscules) : `min`, `max` (2 à 8 arguments), `abs`, `sqrt` (1 argument).
//!   Constante : `pi`. Un nom de variable est `[A-Za-z_][A-Za-z0-9_]*`, la casse compte.
//! - L'évaluateur est **sans unité** : ce sont des nombres purs. L'unité d'une variable est une
//!   étiquette d'affichage, jamais une conversion.
//! - Résultat toujours **fini** : division par zéro, racine d'un négatif, débordement donnent une
//!   erreur typée, jamais `NaN` ni l'infini. `-0` est écrit `0`.
//! - Positions : index de **caractère** (0 = premier), jamais d'octet. Borne : 500 caractères
//!   (`TooLong`), imbrication de 32 niveaux (parenthèses, appels, moins unaires : `TooDeep`).

use std::fmt;

/// Longueur maximale d'une expression, en caractères.
pub const MAX_EXPR_CHARS: usize = 500;
/// Imbrication maximale (parenthèses, appels de fonction, moins unaires enchaînés).
pub const MAX_EXPR_DEPTH: usize = 32;
/// Nombre d'arguments de `min` / `max`.
pub const MIN_CALL_ARGS: usize = 2;
pub const MAX_CALL_ARGS: usize = 8;
/// Mots qui ne peuvent jamais être un nom de variable (comparés sans tenir compte de la casse).
pub const RESERVED_WORDS: [&str; 5] = ["min", "max", "abs", "sqrt", "pi"];

/// Catégorie d'une erreur d'expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprErrorKind {
    Syntax,
    UnknownVariable,
    DivisionByZero,
    /// Hors domaine : racine d'un négatif, résultat ou nombre non fini (débordement).
    Domain,
    /// Dépendance circulaire entre variables (produite par le graphe, pas par l'évaluateur).
    Cycle,
    TooDeep,
    TooLong,
}

/// Erreur localisable : catégorie, position (index de caractère) et message en français.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprError {
    pub kind: ExprErrorKind,
    /// Index de caractère (0 = premier) dans l'expression.
    pub pos: usize,
    /// Nom en cause : variable inconnue, ou chemin du cycle (`A → B → A`).
    pub name: Option<String>,
    pub message: String,
}

impl ExprError {
    pub(crate) fn new(kind: ExprErrorKind, pos: usize, message: impl Into<String>) -> ExprError {
        ExprError {
            kind,
            pos,
            name: None,
            message: message.into(),
        }
    }

    pub(crate) fn with_name(mut self, name: impl Into<String>) -> ExprError {
        self.name = Some(name.into());
        self
    }
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (caractère {})", self.message, self.pos + 1)
    }
}

impl std::error::Error for ExprError {}

/// Référence à une variable dans une expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub name: String,
    /// Index de caractère du premier caractère du nom.
    pub pos: usize,
    /// Longueur du nom, en caractères.
    pub len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Func {
    Min,
    Max,
    Abs,
    Sqrt,
}

impl Func {
    fn from_name(name: &str) -> Option<Func> {
        match name {
            "min" => Some(Func::Min),
            "max" => Some(Func::Max),
            "abs" => Some(Func::Abs),
            "sqrt" => Some(Func::Sqrt),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Func::Min => "min",
            Func::Max => "max",
            Func::Abs => "abs",
            Func::Sqrt => "sqrt",
        }
    }
    fn arity(self) -> (usize, usize) {
        match self {
            Func::Min | Func::Max => (MIN_CALL_ARGS, MAX_CALL_ARGS),
            Func::Abs | Func::Sqrt => (1, 1),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Num(f64),
    Var {
        name: String,
        pos: usize,
    },
    Neg(Box<Node>),
    Bin {
        op: Op,
        left: Box<Node>,
        right: Box<Node>,
        pos: usize,
    },
    Call {
        func: Func,
        args: Vec<Node>,
        pos: usize,
    },
}

/// Expression analysée (syntaxe valide, pas encore évaluée).
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    root: Node,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Semi,
    End,
}

struct Token {
    tok: Tok,
    pos: usize,
}

fn syntax(pos: usize, msg: impl Into<String>) -> ExprError {
    ExprError::new(ExprErrorKind::Syntax, pos, msg)
}

fn lex(chars: &[char]) -> Result<Vec<Token>, ExprError> {
    let mut out = Vec::new();
    let mut i = 0;
    let is_digit = |c: Option<&char>| c.is_some_and(char::is_ascii_digit);
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let single = match c {
            '+' => Some(Tok::Plus),
            '-' => Some(Tok::Minus),
            '*' => Some(Tok::Star),
            '/' => Some(Tok::Slash),
            '(' => Some(Tok::LParen),
            ')' => Some(Tok::RParen),
            ';' => Some(Tok::Semi),
            _ => None,
        };
        if let Some(tok) = single {
            out.push(Token { tok, pos: start });
            i += 1;
        } else if c.is_ascii_digit() || ((c == '.' || c == ',') && is_digit(chars.get(i + 1))) {
            let mut text = String::new();
            while is_digit(chars.get(i)) {
                text.push(chars[i]);
                i += 1;
            }
            if matches!(chars.get(i), Some('.' | ',')) && is_digit(chars.get(i + 1)) {
                if text.is_empty() {
                    text.push('0');
                }
                text.push('.');
                i += 1;
                while is_digit(chars.get(i)) {
                    text.push(chars[i]);
                    i += 1;
                }
            }
            let value: f64 = text
                .parse()
                .map_err(|_| syntax(start, "nombre illisible"))?;
            if !value.is_finite() {
                return Err(ExprError::new(
                    ExprErrorKind::Domain,
                    start,
                    "nombre trop grand",
                ));
            }
            out.push(Token {
                tok: Tok::Num(value),
                pos: start,
            });
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut name = String::new();
            while chars
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                name.push(chars[i]);
                i += 1;
            }
            out.push(Token {
                tok: Tok::Ident(name),
                pos: start,
            });
        } else {
            return Err(syntax(start, format!("caractère « {c} » non permis")));
        }
    }
    out.push(Token {
        tok: Tok::End,
        pos: chars.len(),
    });
    Ok(out)
}

struct Parser {
    toks: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.toks[self.at]
    }

    fn next(&mut self) -> &Token {
        let i = self.at;
        if self.at + 1 < self.toks.len() {
            self.at += 1;
        }
        &self.toks[i]
    }

    fn deeper(depth: usize, pos: usize) -> Result<usize, ExprError> {
        if depth + 1 > MAX_EXPR_DEPTH {
            Err(ExprError::new(
                ExprErrorKind::TooDeep,
                pos,
                format!("expression trop imbriquée (au plus {MAX_EXPR_DEPTH} niveaux)"),
            ))
        } else {
            Ok(depth + 1)
        }
    }

    fn expr(&mut self, depth: usize) -> Result<Node, ExprError> {
        let mut left = self.term(depth)?;
        loop {
            let (op, pos) = match self.peek().tok {
                Tok::Plus => (Op::Add, self.peek().pos),
                Tok::Minus => (Op::Sub, self.peek().pos),
                _ => return Ok(left),
            };
            self.next();
            let right = self.term(depth)?;
            left = Node::Bin {
                op,
                left: Box::new(left),
                right: Box::new(right),
                pos,
            };
        }
    }

    fn term(&mut self, depth: usize) -> Result<Node, ExprError> {
        let mut left = self.unary(depth)?;
        loop {
            let (op, pos) = match self.peek().tok {
                Tok::Star => (Op::Mul, self.peek().pos),
                Tok::Slash => (Op::Div, self.peek().pos),
                _ => return Ok(left),
            };
            self.next();
            let right = self.unary(depth)?;
            left = Node::Bin {
                op,
                left: Box::new(left),
                right: Box::new(right),
                pos,
            };
        }
    }

    fn unary(&mut self, depth: usize) -> Result<Node, ExprError> {
        if self.peek().tok == Tok::Minus {
            let pos = self.peek().pos;
            let depth = Self::deeper(depth, pos)?;
            self.next();
            return Ok(Node::Neg(Box::new(self.unary(depth)?)));
        }
        self.primary(depth)
    }

    fn primary(&mut self, depth: usize) -> Result<Node, ExprError> {
        let (tok, pos) = {
            let t = self.next();
            (t.tok.clone(), t.pos)
        };
        match tok {
            Tok::Num(v) => Ok(Node::Num(v)),
            Tok::LParen => {
                let depth = Self::deeper(depth, pos)?;
                let inner = self.expr(depth)?;
                match self.peek().tok {
                    Tok::RParen => {
                        self.next();
                        Ok(inner)
                    }
                    _ => Err(syntax(pos, "parenthèse ouvrante jamais refermée")),
                }
            }
            Tok::Ident(name) => {
                if self.peek().tok == Tok::LParen {
                    self.call(&name, pos, depth)
                } else if name == "pi" {
                    Ok(Node::Num(std::f64::consts::PI))
                } else if Func::from_name(&name).is_some() {
                    Err(syntax(
                        pos,
                        format!("« {name} » est une fonction : écrivez {name}(…)"),
                    ))
                } else {
                    Ok(Node::Var { name, pos })
                }
            }
            Tok::End => Err(syntax(
                pos,
                "expression incomplète : une valeur est attendue",
            )),
            Tok::RParen => Err(syntax(pos, "parenthèse fermante sans ouvrante")),
            Tok::Semi => Err(syntax(
                pos,
                "« ; » ne sépare que les arguments d'une fonction",
            )),
            Tok::Plus | Tok::Star | Tok::Slash | Tok::Minus => {
                Err(syntax(pos, "une valeur est attendue avant cet opérateur"))
            }
        }
    }

    fn call(&mut self, name: &str, pos: usize, depth: usize) -> Result<Node, ExprError> {
        let Some(func) = Func::from_name(name) else {
            let why = if name == "pi" {
                "« pi » est une constante, pas une fonction".to_string()
            } else {
                format!("fonction « {name} » inconnue (min, max, abs, sqrt)")
            };
            return Err(syntax(pos, why));
        };
        let open = self.next().pos; // la parenthèse ouvrante
        let depth = Self::deeper(depth, open)?;
        let mut args = Vec::new();
        if self.peek().tok != Tok::RParen {
            loop {
                args.push(self.expr(depth)?);
                match self.peek().tok {
                    Tok::Semi => {
                        self.next();
                    }
                    Tok::RParen => break,
                    _ => return Err(syntax(open, "parenthèse ouvrante jamais refermée")),
                }
            }
        }
        self.next(); // la parenthèse fermante
        let (lo, hi) = func.arity();
        if args.len() < lo || args.len() > hi {
            let wanted = if lo == hi {
                format!("{lo} argument")
            } else {
                format!("de {lo} à {hi} arguments")
            };
            return Err(syntax(
                pos,
                format!(
                    "{}() attend {wanted} (reçu {}) ; séparez les arguments par « ; »",
                    func.name(),
                    args.len()
                ),
            ));
        }
        Ok(Node::Call { func, args, pos })
    }
}

/// Analyse une expression (aucune variable n'est résolue ici).
pub fn parse(text: &str) -> Result<Expr, ExprError> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > MAX_EXPR_CHARS {
        return Err(ExprError::new(
            ExprErrorKind::TooLong,
            MAX_EXPR_CHARS,
            format!("expression trop longue (au plus {MAX_EXPR_CHARS} caractères)"),
        ));
    }
    let toks = lex(&chars)?;
    if toks[0].tok == Tok::End {
        return Err(syntax(0, "expression vide"));
    }
    let mut parser = Parser { toks, at: 0 };
    let root = parser.expr(0)?;
    let t = parser.peek();
    match t.tok {
        Tok::End => Ok(Expr { root }),
        Tok::RParen => Err(syntax(t.pos, "parenthèse fermante sans ouvrante")),
        Tok::Num(_) | Tok::Ident(_) | Tok::LParen => {
            Err(syntax(t.pos, "un opérateur (+ - * /) est attendu ici"))
        }
        _ => Err(syntax(t.pos, "symbole inattendu")),
    }
}

fn clean(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

fn finite(v: f64, pos: usize) -> Result<f64, ExprError> {
    if v.is_finite() {
        Ok(clean(v))
    } else {
        Err(ExprError::new(
            ExprErrorKind::Domain,
            pos,
            "résultat hors limites (débordement)",
        ))
    }
}

fn eval_node(node: &Node, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError> {
    match node {
        Node::Num(v) => Ok(*v),
        Node::Var { name, pos } => lookup(name).ok_or_else(|| {
            ExprError::new(
                ExprErrorKind::UnknownVariable,
                *pos,
                format!("variable « {name} » inconnue"),
            )
            .with_name(name.clone())
        }),
        Node::Neg(inner) => Ok(clean(-eval_node(inner, lookup)?)),
        Node::Bin {
            op,
            left,
            right,
            pos,
        } => {
            let l = eval_node(left, lookup)?;
            let r = eval_node(right, lookup)?;
            let v = match op {
                Op::Add => l + r,
                Op::Sub => l - r,
                Op::Mul => l * r,
                Op::Div => {
                    if r == 0.0 {
                        return Err(ExprError::new(
                            ExprErrorKind::DivisionByZero,
                            *pos,
                            "division par zéro",
                        ));
                    }
                    l / r
                }
            };
            finite(v, *pos)
        }
        Node::Call { func, args, pos } => {
            let mut values = Vec::with_capacity(args.len());
            for a in args {
                values.push(eval_node(a, lookup)?);
            }
            let v = match func {
                Func::Abs => values[0].abs(),
                Func::Sqrt => {
                    if values[0] < 0.0 {
                        return Err(ExprError::new(
                            ExprErrorKind::Domain,
                            *pos,
                            "racine carrée d'un nombre négatif",
                        ));
                    }
                    values[0].sqrt()
                }
                Func::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
                Func::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            };
            finite(v, *pos)
        }
    }
}

fn collect_refs(node: &Node, out: &mut Vec<Reference>) {
    match node {
        Node::Num(_) => {}
        Node::Var { name, pos } => out.push(Reference {
            name: name.clone(),
            pos: *pos,
            len: name.chars().count(),
        }),
        Node::Neg(inner) => collect_refs(inner, out),
        Node::Bin { left, right, .. } => {
            collect_refs(left, out);
            collect_refs(right, out);
        }
        Node::Call { args, .. } => args.iter().for_each(|a| collect_refs(a, out)),
    }
}

impl Expr {
    /// Valeur de l'expression ; `lookup` donne la valeur d'une variable (ou `None` si inconnue).
    pub fn eval(&self, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError> {
        let v = eval_node(&self.root, lookup)?;
        finite(v, 0)
    }

    /// Références à des variables, dans l'ordre de lecture (un nom répété l'est aussi).
    pub fn references(&self) -> Vec<Reference> {
        let mut out = Vec::new();
        collect_refs(&self.root, &mut out);
        out
    }
}

/// Analyse puis évalue.
pub fn evaluate(text: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError> {
    parse(text)?.eval(lookup)
}

/// Réécrit les références à `from` en `to` (jetons entiers : `B` ne touche ni `B2`, ni `AB`, ni
/// un nom de fonction). Le reste du texte (espaces compris) est conservé tel quel.
pub fn rename_reference(text: &str, from: &str, to: &str) -> Result<String, ExprError> {
    let refs = parse(text)?.references();
    let mut chars: Vec<char> = text.chars().collect();
    for r in refs.iter().rev().filter(|r| r.name == from) {
        chars.splice(r.pos..r.pos + r.len, to.chars());
    }
    Ok(chars.into_iter().collect())
}
