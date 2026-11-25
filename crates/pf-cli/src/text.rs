//! Messages de la ligne de commande : clés `cli.*` du catalogue de `pf-i18n` (FR et EN).

use pf_i18n::{lookup, Lang};

pub fn m(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = lookup(lang, key);
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}
