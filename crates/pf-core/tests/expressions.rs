//! US-03.1a — évaluateur d'expressions : valeurs écrites à la main, erreurs (catégorie et
//! position en index de caractère), bornes de longueur et de profondeur, réécriture de références.

use pf_core::expr::{
    evaluate, parse, rename_reference, ExprError, ExprErrorKind, MAX_EXPR_CHARS, MAX_EXPR_DEPTH,
};
use ExprErrorKind::*;

fn ev(text: &str) -> Result<f64, ExprError> {
    evaluate(text, &|_| None)
}

fn ok(text: &str, expected: f64) {
    let v = ev(text).unwrap_or_else(|e| panic!("« {text} » : {e}"));
    assert_eq!(v, expected, "« {text} »");
}

fn err(text: &str, kind: ExprErrorKind, pos: usize) -> ExprError {
    let e = ev(text).expect_err(&format!("« {text} » devrait échouer"));
    assert_eq!((e.kind, e.pos), (kind, pos), "« {text} » : {e}");
    e
}

// 6.283185307179586 est écrit à la main (2 pi) : le littéral est voulu.
#[allow(clippy::approx_constant)]
#[test]
fn valeurs_a_la_main() {
    ok("2+3*4", 14.0);
    ok("(2+3)*4", 20.0);
    ok("-(2+3)", -5.0);
    ok("min(3;1;2)", 1.0);
    ok("max(3;1;2)", 3.0);
    ok("sqrt(16)", 4.0);
    ok("abs(-7,5)", 7.5);
    ok("abs(-7.5)", 7.5);
    ok("10/4", 2.5);
    ok("1,5*2", 3.0);
    ok("pi*2", 6.283185307179586);
    ok("8-3-2", 3.0); // associativité à gauche
    ok("8/2/2", 2.0);
    ok("2*3+4*5", 26.0);
    ok("--3", 3.0);
    ok("2*-3", -6.0);
    ok("2--3", 5.0);
    ok(".5+,5", 1.0);
    ok("  1 +  2  ", 3.0);
    ok("\u{a0}1\t+\n1", 2.0);
    ok("min(1,5;2)", 1.5); // la virgule est une décimale
    ok("max(1,5;2)", 2.0);
    ok("min(max(1;2);3)", 2.0);
    ok("min(1+1;3*3)", 2.0);
    ok("min(1;2;3;4;5;6;7;8)", 1.0);
    ok("max(1;2;3;4;5;6;7;8)", 8.0);
    ok("1/3*3", 1.0);
    ok("920/4+40", 270.0);
}

#[test]
fn moins_zero_est_ecrit_zero() {
    for t in ["0*-1", "-0", "0/-5", "abs(-0)", "-0+0*5"] {
        let v = ev(t).unwrap();
        assert_eq!(v, 0.0);
        assert!(v.is_sign_positive(), "« {t} » = -0");
    }
}

#[test]
fn erreurs_avec_categorie_et_position() {
    err("1/0", DivisionByZero, 1);
    err("1/(2-2)", DivisionByZero, 1);
    err("4+8/0", DivisionByZero, 3);
    err("sqrt(-1)", Domain, 0);
    err("2*sqrt(1-2)", Domain, 2);
    err("2+", Syntax, 2);
    err("(1+2", Syntax, 0);
    err("min(1;2", Syntax, 3);
    err("foo", UnknownVariable, 0);
    err("1+foo*2", UnknownVariable, 2);
    err("1e999", Syntax, 1); // pas de notation scientifique
    err("min()", Syntax, 0);
    err("abs(1;2)", Syntax, 0);
    err("abs()", Syntax, 0);
    err("min(1)", Syntax, 0);
    err("min(1,2)", Syntax, 0); // un seul argument 1,2 : la virgule est une décimale
    err("min(1;2;3;4;5;6;7;8;9)", Syntax, 0);
    err("", Syntax, 0);
    err("   ", Syntax, 0);
    err("1 2", Syntax, 2);
    err("1,2,3", Syntax, 3);
    err("1.2.3", Syntax, 3);
    err("1+)", Syntax, 2);
    err(")", Syntax, 0);
    err("(1))", Syntax, 3);
    err("()", Syntax, 1);
    err("+1", Syntax, 0);
    err("*2", Syntax, 0);
    err("2**3", Syntax, 2);
    err("2^3", Syntax, 1);
    err("3 $ 4", Syntax, 2);
    err("1+é", Syntax, 2);
    err("1;2", Syntax, 1);
    err("min", Syntax, 0);
    err("sqrt+1", Syntax, 0);
    err("foo(1)", Syntax, 0);
    err("MIN(1;2)", Syntax, 0); // les fonctions s'écrivent en minuscules
    err("pi(1)", Syntax, 0);
    err("5.", Syntax, 1);
    err("PI", UnknownVariable, 0); // seule `pi` est la constante
}

#[test]
fn position_en_caracteres_pas_en_octets() {
    // L'espace insécable occupe 2 octets : « / » est au caractère 2, pas à l'octet 3.
    err("\u{a0}1/0", DivisionByZero, 2);
    err("\u{a0}\u{a0}foo", UnknownVariable, 2);
}

#[test]
fn debordement_refuse_jamais_inf_ni_nan() {
    let big = "9".repeat(400);
    err(&big, Domain, 0);
    let d = "9".repeat(200);
    err(&format!("{d}*{d}"), Domain, 200);
    // 1e150 × 1e150 reste fini ; le troisième facteur déborde, à la seconde étoile.
    let c = "9".repeat(150);
    err(&format!("{c}*{c}*{c}"), Domain, 301);
    err(&format!("-{d}*{d}"), Domain, 201);
    // Un grand nombre fini reste accepté.
    let v = ev(&format!("{d}*1")).unwrap();
    assert!(v.is_finite() && v > 1e199);
}

#[test]
fn longueur_et_profondeur() {
    // 500 caractères : accepté ; 501 : TooLong, position = la borne.
    let at_limit = format!("1{}", "+1".repeat(249)) + " ";
    assert_eq!(at_limit.chars().count(), MAX_EXPR_CHARS);
    ok(&at_limit, 250.0);
    let over = format!("{at_limit} ");
    let e = err(&over, TooLong, MAX_EXPR_CHARS);
    assert!(e.message.contains("500"));
    // 1000 parenthèses ouvrantes : trop long avant toute récursion.
    err(&"(".repeat(1000), TooLong, MAX_EXPR_CHARS);
    // Imbrication : 32 niveaux acceptés, le 33e est refusé à sa position.
    let nest = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
    ok(&nest(MAX_EXPR_DEPTH), 1.0);
    err(&nest(MAX_EXPR_DEPTH + 1), TooDeep, MAX_EXPR_DEPTH);
    err(&nest(200), TooDeep, MAX_EXPR_DEPTH);
    ok(&format!("{}1", "-".repeat(MAX_EXPR_DEPTH)), 1.0);
    err(
        &format!("{}1", "-".repeat(MAX_EXPR_DEPTH + 1)),
        TooDeep,
        MAX_EXPR_DEPTH,
    );
    let calls = |n: usize| format!("{}1{}", "abs(".repeat(n), ")".repeat(n));
    ok(&calls(MAX_EXPR_DEPTH), 1.0);
    // Le 33e appel : sa parenthèse ouvrante est au caractère 32 × 4 + 3.
    err(&calls(MAX_EXPR_DEPTH + 1), TooDeep, MAX_EXPR_DEPTH * 4 + 3);
}

#[test]
fn references_et_variables() {
    let e = parse("B+2*C-B").unwrap();
    let refs: Vec<_> = e
        .references()
        .into_iter()
        .map(|r| (r.name, r.pos, r.len))
        .collect();
    assert_eq!(
        refs,
        vec![("B".into(), 0, 1), ("C".into(), 4, 1), ("B".into(), 6, 1)]
    );
    let lookup = |n: &str| match n {
        "B" => Some(10.0),
        "C" => Some(4.0),
        _ => None,
    };
    assert_eq!(e.eval(&lookup), Ok(10.0 + 8.0 - 10.0));
    assert_eq!(evaluate("b", &lookup).unwrap_err().kind, UnknownVariable); // la casse compte
                                                                           // Une fonction n'est pas une référence.
    assert_eq!(parse("min(B;pi)").unwrap().references().len(), 1);
}

#[test]
fn reecriture_de_references_aux_frontieres_de_mot() {
    let r = |t: &str, from: &str, to: &str| rename_reference(t, from, to).unwrap();
    assert_eq!(r("A + B*B2 - AB", "B", "X"), "A + X*B2 - AB");
    assert_eq!(r("B+1", "B", "B2"), "B2+1");
    assert_eq!(r("B22+B2+1", "B2", "Z"), "B22+Z+1");
    assert_eq!(r("min(B;2)*B", "B", "LongNom"), "min(LongNom;2)*LongNom");
    assert_eq!(r("1+2", "B", "X"), "1+2");
    assert_eq!(r(" B_1 + B ", "B", "C"), " B_1 + C ");
    assert!(rename_reference("1+", "B", "C").is_err());
}
