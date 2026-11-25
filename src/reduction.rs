//! The reduction table (*visszavezetési táblázat*): for every programming theorem of a
//! specification (`SZUMMA`, `DARAB`, `KERES`, …), the theorem's template next to the line of the
//! specification that uses it, and what every part of the template is in this task:
//!
//! ```text
//! /* Visszavezetés
//! (van, ind) = KERES(i=e..u, T(i))
//! (found, nap) = SEARCH(i=1..n-(k-1), nicewind(i))
//! van -> found
//! ind -> nap
//! i -> i
//! e -> 1
//! u -> n-(k-1)
//! T(i) -> nicewind(i)
//! */
//! ```
//!
//! The table is a block comment, so the specification stays valid with it ([`apply`] writes it
//! under the specification, and replaces the one it wrote before).

use crate::parser::{Expr, ExprKind, Spec};

/// First line of the comment [`apply`] writes. It is how an earlier table is found again.
pub const MARK: &str = "/* Visszavezetés";

/// One theorem: its template and what goes into it.
struct Template {
    /// The template as the course writes it (`s = SZUMMA(i=e..u, f(i))`).
    line: &'static str,
    /// The names the template gives to its results.
    results: &'static [&'static str],
    /// The names the template gives to the arguments after the range.
    args: &'static [&'static str],
}

const fn t(
    line: &'static str,
    results: &'static [&'static str],
    args: &'static [&'static str],
) -> Template {
    Template {
        line,
        results,
        args,
    }
}

/// The template of the theorem `name` (as the parser calls it) with `args` after the range.
fn template(name: &str, args: &[String]) -> Option<Template> {
    Some(match (name, args.len()) {
        ("SUM", 1) => t("s = SZUMMA(i=e..u, f(i))", &["s"], &["f(i)"]),
        ("SUM", 2) => t("s = SZUMMA(i=e..u, f(i), T(i))", &["s"], &["f(i)", "T(i)"]),
        ("SUM", 3) if args[1] == "0" && args[2] == "+" => {
            t("s = SZUMMA(i=e..u, f(i), 0, +)", &["s"], &["f(i)"])
        }
        ("SUM", 3) => t(
            "s = SZUMMA(i=e..u, f(i), kezd, add)",
            &["s"],
            &["f(i)", "kezd", "add"],
        ),
        ("COUNT" | "DARAB", 1) => t("db = DARAB(i=e..u, T(i))", &["db"], &["T(i)"]),
        ("MAX", 1) => t(
            "(maxind, maxért) = MAX(i=e..u, f(i))",
            &["maxind", "maxért"],
            &["f(i)"],
        ),
        ("MIN", 1) => t(
            "(minind, minért) = MIN(i=e..u, f(i))",
            &["minind", "minért"],
            &["f(i)"],
        ),
        ("CONDMAX", 2) => t(
            "(van, maxind, maxért) = FELTMAX(i=e..u, f(i), T(i))",
            &["van", "maxind", "maxért"],
            &["f(i)", "T(i)"],
        ),
        ("CONDMIN", 2) => t(
            "(van, minind, minért) = FELTMIN(i=e..u, f(i), T(i))",
            &["van", "minind", "minért"],
            &["f(i)", "T(i)"],
        ),
        ("SEARCH", 1) => t(
            "(van, ind) = KERES(i=e..u, T(i))",
            &["van", "ind"],
            &["T(i)"],
        ),
        ("EXISTS", 1) => t("van = VAN(i=e..u, T(i))", &["van"], &["T(i)"]),
        ("FORALL", 1) => t("mind = MIND(i=e..u, T(i))", &["mind"], &["T(i)"]),
        ("SELECT", 1) => t("ind = KIVÁLASZT(i>=e, T(i))", &["ind"], &["T(i)"]),
        ("COPY", 1) => t("y = MÁSOL(i=e..u, f(i))", &["y"], &["f(i)"]),
        ("MIS", 1) => t(
            "(db, y) = KIVÁLOGAT(i=e..u, T(i), f(i))",
            &["db", "y"],
            &["T(i)"],
        ),
        ("MIS", 2) => t(
            "(db, y) = KIVÁLOGAT(i=e..u, T(i), f(i))",
            &["db", "y"],
            &["T(i)", "f(i)"],
        ),
        _ => return None,
    })
}

/// A theorem call, with what it is assigned to.
struct Use<'a> {
    mac: &'a Expr,
    /// The line of the specification that has it.
    stmt: String,
    /// What the results are bound to: one text per slot (`(found, nap)`), empty for a skipped one.
    to: Vec<String>,
    /// `.2`, `.maxind`, …: only that part of the result is used.
    part: Option<String>,
}

/// The theorem in `e`, if `e` is one, or one of its parts (`MAX(…).2`).
fn theorem(e: &Expr) -> Option<(&Expr, Option<&str>)> {
    match &*e.kind {
        ExprKind::Macro { .. } => Some((e, None)),
        ExprKind::Field(inner, part) if matches!(&*inner.kind, ExprKind::Macro { .. }) => {
            Some((inner, Some(part.as_str())))
        }
        _ => None,
    }
}

fn slots(spec: &Spec, lhs: &Expr) -> Vec<String> {
    match &*lhs.kind {
        ExprKind::Tuple(s) => s
            .iter()
            .map(|e| {
                e.as_ref()
                    .map(|e| spec.text(e).to_string())
                    .unwrap_or_default()
            })
            .collect(),
        _ => vec![spec.text(lhs).to_string()],
    }
}

fn collect<'a>(spec: &'a Spec, e: &'a Expr, out: &mut Vec<Use<'a>>) {
    if let ExprKind::Cmp(operands, ops) = &*e.kind
        && operands.len() == 2
        && ops.as_slice() == ["="]
        && let Some((mac, part)) = theorem(&operands[1])
    {
        out.push(Use {
            mac,
            stmt: spec.text(e).to_string(),
            to: slots(spec, &operands[0]),
            part: part.map(str::to_string),
        });
        // a theorem inside this one's arguments, or inside the left side
        for c in mac.children() {
            collect(spec, c, out);
        }
        collect(spec, &operands[0], out);
        return;
    }
    if let ExprKind::Macro { .. } = &*e.kind {
        out.push(Use {
            mac: e,
            stmt: spec.text(e).to_string(),
            to: vec![],
            part: None,
        });
    }
    for c in e.children() {
        collect(spec, c, out);
    }
}

/// Which result of the template `names` a part of the result (`2`, `maxind`, `found`) is.
fn part_index(names: &[&str], part: &str) -> Option<usize> {
    if let Ok(n) = part.parse::<usize>() {
        return n.checked_sub(1).filter(|k| *k < names.len());
    }
    match part {
        "found" => names.iter().position(|n| *n == "van"),
        "cnt" => names.iter().position(|n| *n == "db"),
        "items" => names.iter().position(|n| *n == "y"),
        "ind" | "maxind" | "minind" => names.iter().position(|n| n.ends_with("ind")),
        "val" | "maxval" | "minval" | "value" => names.iter().position(|n| n.ends_with("ért")),
        _ => None,
    }
}

/// The lines of one theorem's table.
fn block(spec: &Spec, u: &Use) -> Option<String> {
    let ExprKind::Macro {
        name,
        var,
        lo,
        hi,
        args,
    } = &*u.mac.kind
    else {
        return None;
    };
    let args: Vec<String> = args.iter().map(|a| spec.text(a).to_string()).collect();
    let tpl = template(name, &args)?;
    let mut lines = vec![tpl.line.to_string(), u.stmt.clone()];
    // the results
    match &u.part {
        Some(part) => {
            if let (Some(k), [to]) = (part_index(tpl.results, part), u.to.as_slice())
                && !to.is_empty()
            {
                lines.push(format!("{} -> {to}", tpl.results[k]));
            }
        }
        None => {
            for (r, to) in tpl.results.iter().zip(&u.to) {
                if !to.is_empty() {
                    lines.push(format!("{r} -> {to}"));
                }
            }
        }
    }
    // the interval
    lines.push(format!("i -> {var}"));
    lines.push(format!("e -> {}", spec.text(lo)));
    if let Some(hi) = hi {
        lines.push(format!("u -> {}", spec.text(hi)));
    }
    // the arguments
    for (k, a) in args.iter().enumerate() {
        if let Some(abstract_name) = tpl.args.get(k) {
            lines.push(format!("{abstract_name} -> {a}"));
        }
    }
    // a selection without a value function collects the indexes
    if name == "MIS" && args.len() == 1 {
        lines.push(format!("f(i) -> {var}"));
    }
    Some(lines.join("\n"))
}

/// The reduction table of a specification: its theorems in the postcondition, then the ones in
/// its functions. `None`: it has none.
pub fn table(spec: &Spec) -> Option<String> {
    let mut found: Vec<Use> = vec![];
    collect(spec, &spec.post, &mut found);
    let mut funcs: Vec<_> = spec.funcs.iter().collect();
    funcs.sort_by_key(|(_, f)| f.body.start);
    let mut owned: Vec<(String, &Expr)> = vec![];
    for (name, f) in funcs {
        let call = format!("{name}({})", f.params.join(", "));
        owned.push((call, &f.body));
    }
    let mut from_funcs: Vec<Use> = vec![];
    for (call, body) in &owned {
        let mut inner = vec![];
        if let Some((mac, part)) = theorem(body) {
            inner.push(Use {
                mac,
                stmt: format!("{call} = {}", spec.text(body)),
                to: vec![call.clone()],
                part: part.map(str::to_string),
            });
            for c in mac.children() {
                collect(spec, c, &mut inner);
            }
        } else {
            collect(spec, body, &mut inner);
        }
        from_funcs.extend(inner);
    }
    found.extend(from_funcs);
    let blocks: Vec<String> = found.iter().filter_map(|u| block(spec, u)).collect();
    if blocks.is_empty() {
        return None;
    }
    Some(format!("{MARK}\n{}\n*/", blocks.join("\n\n")))
}

/// `spec_text` with the table `block` under it. The table an earlier call wrote (the last
/// comment of the text, if it starts with [`MARK`]) is replaced, never doubled.
pub fn apply(spec_text: &str, block: &str) -> String {
    let mut text = spec_text.to_string();
    if let Some(at) = text.rfind(MARK)
        && let Some(end) = text[at..].find("*/")
        && text[at + end + 2..].trim().is_empty()
    {
        text.truncate(at);
    }
    let mut text = text.trim_end().to_string();
    text.push_str("\n\n");
    text.push_str(block);
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    const WIND: &str = "In: n∈Z, k∈N, szel∈N[1..n]\nAux: found∈L\nOut: nap∈Z\nFn: nicewind: N->L, nicewind(x) = FORALL(i=1..k, 10≤szel[i+x-1]≤100)\nPre: 1≤n≤1000 and 1≤k≤10 and ∀i∈[1..n]:(0≤szel[i]≤200)\nPost: (found, nap) = SEARCH(i=1..n-(k-1), nicewind(i))\n  and not(found) -> nap = -1\n";

    fn table_of(src: &str) -> String {
        table(&parse(src).unwrap_or_else(|e| panic!("{}", e[0].title))).unwrap()
    }

    #[test]
    fn a_search_with_a_function_inside() {
        assert_eq!(
            table_of(WIND),
            "/* Visszavezetés
(van, ind) = KERES(i=e..u, T(i))
(found, nap) = SEARCH(i=1..n-(k-1), nicewind(i))
van -> found
ind -> nap
i -> i
e -> 1
u -> n-(k-1)
T(i) -> nicewind(i)

mind = MIND(i=e..u, T(i))
nicewind(x) = FORALL(i=1..k, 10≤szel[i+x-1]≤100)
mind -> nicewind(x)
i -> i
e -> 1
u -> k
T(i) -> 10≤szel[i+x-1]≤100
*/"
        );
    }

    #[test]
    fn every_theorem_has_a_table() {
        let cases = [
            (
                "s=SZUMMA(i=1..n, t[i])",
                "s = SZUMMA(i=e..u, f(i))",
                "Ki: s∈Z",
            ),
            (
                "s=SZUMMA(i=1..n, t[i], 0, +)",
                "s = SZUMMA(i=e..u, f(i), 0, +)",
                "Ki: s∈Z",
            ),
            (
                "s=SZUMMA(i=1..n, t[i], 1, *)",
                "s = SZUMMA(i=e..u, f(i), kezd, add)",
                "Ki: s∈Z",
            ),
            (
                "db=DARAB(i=1..n, t[i]>0)",
                "db = DARAB(i=e..u, T(i))",
                "Ki: db∈N",
            ),
            (
                "(mi,mx)=MAX(i=1..n, t[i])",
                "(maxind, maxért) = MAX(i=e..u, f(i))",
                "Ki: mi∈N, mx∈Z",
            ),
            (
                "(mi,mx)=MIN(i=1..n, t[i])",
                "(minind, minért) = MIN(i=e..u, f(i))",
                "Ki: mi∈N, mx∈Z",
            ),
            (
                "(v,mi,mx)=FELTMAX(i=1..n, t[i], t[i]>0)",
                "(van, maxind, maxért) = FELTMAX(i=e..u, f(i), T(i))",
                "Ki: v∈L, mi∈N, mx∈Z",
            ),
            (
                "(v,mi,mx)=FELTMIN(i=1..n, t[i], t[i]>0)",
                "(van, minind, minért) = FELTMIN(i=e..u, f(i), T(i))",
                "Ki: v∈L, mi∈N, mx∈Z",
            ),
            (
                "(v,ind)=KERES(i=1..n, t[i]>0)",
                "(van, ind) = KERES(i=e..u, T(i))",
                "Ki: v∈L, ind∈N",
            ),
            (
                "v=VAN(i=1..n, t[i]>0)",
                "van = VAN(i=e..u, T(i))",
                "Ki: v∈L",
            ),
            (
                "v=MIND(i=1..n, t[i]>0)",
                "mind = MIND(i=e..u, T(i))",
                "Ki: v∈L",
            ),
            (
                "ind=KIVÁLASZT(i>=1, t[i]>n)",
                "ind = KIVÁLASZT(i>=e, T(i))",
                "Ki: ind∈N",
            ),
            (
                "y=MÁSOL(i=1..n, t[i])",
                "y = MÁSOL(i=e..u, f(i))",
                "Ki: y∈Z[1..n]",
            ),
            (
                "(db,y)=KIVÁLOGAT(i=1..n, t[i]>0, t[i])",
                "(db, y) = KIVÁLOGAT(i=e..u, T(i), f(i))",
                "Ki: db∈N, y∈Z[1..db]",
            ),
        ];
        for (post, line, out) in cases {
            let src = format!("Be: n∈N, t∈Z[1..n]\n{out}\nEf: -\nUf: {post}\n");
            let table = table_of(&src);
            assert!(table.contains(line), "{post}\n{table}");
            assert!(table.contains(post), "{post}\n{table}");
            assert!(table.contains("i -> i"), "{post}\n{table}");
        }
    }

    #[test]
    fn a_part_of_a_result_and_a_skipped_one() {
        let src = "Be: n∈N, t∈Z[1..n]\nKi: m∈Z, k∈N\nEf: n>0\nUf: m=MAX(i=1..n, t[i]).2 és (k,)=MAX(i=1..n, t[i])\n";
        let t = table_of(src);
        assert!(t.contains("maxért -> m\n"), "{t}");
        assert!(t.contains("maxind -> k\n"), "{t}");
        assert!(!t.contains("maxért -> k"), "{t}");
    }

    #[test]
    fn no_theorem_no_table() {
        let spec = parse("Be: n∈N\nKi: m∈N\nEf: -\nUf: m=n+1\n").unwrap();
        assert!(table(&spec).is_none());
    }

    #[test]
    fn the_table_is_written_once_and_the_spec_stays_valid() {
        let base = "Be: n∈N, t∈Z[1..n]\nKi: db∈N\nEf: -\nUf: db=DARAB(i=1..n, t[i]>0)\n";
        let spec = parse(base).unwrap();
        let block = table(&spec).unwrap();
        let once = apply(base, &block);
        assert!(once.starts_with(base.trim_end()));
        assert_eq!(once.matches(MARK).count(), 1);
        assert!(parse(&once).is_ok(), "{once}");
        // again: replaced, not doubled; the same text comes out
        assert_eq!(apply(&once, &block), once);
        // a comment of the user's own is kept
        let own = format!("{base}/* my notes */\n");
        let both = apply(&own, &block);
        assert!(both.contains("/* my notes */") && both.contains(MARK));
        assert_eq!(apply(&both, &block), both);
    }
}
