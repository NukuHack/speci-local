//! Decisions that follow each other on the same condition, or on a condition and its opposite,
//! become one decision: `[if c] A` `[if nem c] B` → `[if c] A [else] B`, and
//! `[if c] A` `[if c] B` → `[if c] A B`. It is only done when the first decision leaves what
//! the condition reads alone, so that the second one would have decided the same way.

use super::model::{Kind, Node};
use crate::parser::{Expr, ExprKind, Fragment, parse_fragment};

/// How the condition of a decision relates to the one before it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Rel {
    Same,
    Opposite,
}

/// `text` without spaces and without parentheses around all of it.
fn norm(text: &str) -> String {
    let mut s: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    while s.starts_with('(') && s.ends_with(')') && encloses(&s) {
        s = s[1..s.len() - 1].to_string();
    }
    s
}

/// Do the first and the last parenthesis of `s` belong together?
fn encloses(s: &str) -> bool {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && i + 1 < s.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

fn parsed(t: &str) -> Option<Expr> {
    match parse_fragment(t, 0, t.len()) {
        Ok(Fragment::Expr(e)) => Some(e),
        _ => None,
    }
}

pub fn same_cond(a: &str, b: &str) -> bool {
    norm(a) == norm(b)
}

const NEGATED: [(&str, &str); 6] = [
    ("<", ">="),
    (">", "<="),
    ("=", "!="),
    ("|", "∤"),
    ("<=", ">"),
    (">=", "<"),
];

fn negated(op: &str) -> Option<&'static str> {
    NEGATED.iter().find_map(|&(a, b)| match op {
        _ if op == a => Some(b),
        _ if op == b => Some(a),
        _ => None,
    })
}

fn mirrored(op: &str) -> Option<&'static str> {
    Some(match op {
        "<" => ">",
        ">" => "<",
        "<=" => ">=",
        ">=" => "<=",
        "=" => "=",
        "!=" => "!=",
        _ => return None,
    })
}

/// `c` and `nem c` (either way round), or `x<y` and `x≥y`.
pub fn opposite_cond(a: &str, b: &str) -> bool {
    let (Some(ea), Some(eb)) = (parsed(a), parsed(b)) else {
        return false;
    };
    let at = |s: &str, e: &Expr| norm(&s[e.start..e.end]);
    match (&*ea.kind, &*eb.kind) {
        (ExprKind::Not(x), _) if at(a, x) == norm(b) => true,
        (_, ExprKind::Not(y)) if at(b, y) == norm(a) => true,
        (ExprKind::Cmp(v, p), ExprKind::Cmp(u, q)) if v.len() == 2 && u.len() == 2 => {
            let straight = at(a, &v[0]) == at(b, &u[0]) && at(a, &v[1]) == at(b, &u[1]);
            let swapped = at(a, &v[0]) == at(b, &u[1]) && at(a, &v[1]) == at(b, &u[0]);
            straight && negated(p[0]) == Some(q[0])
                || swapped && mirrored(p[0]).and_then(negated) == Some(q[0])
        }
        _ => false,
    }
}

/// The relation of two conditions, if there is one.
pub fn relation(a: &str, b: &str) -> Option<Rel> {
    if same_cond(a, b) {
        Some(Rel::Same)
    } else if opposite_cond(a, b) {
        Some(Rel::Opposite)
    } else {
        None
    }
}

/// The names in a text (`a[i]+f(x)` → a, i, f, x).
pub fn names_in(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else if !cur.is_empty() {
            if !cur.starts_with(|c: char| c.is_ascii_digit()) {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
        }
    }
    out
}

/// The variables the steps give values to (`x:=…`, `t[i]:=…`, `(a,b):=…`, `i=1..n`).
pub fn assigned_names(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match &n.kind {
            Kind::Statement(t) => {
                if let Some((lhs, _)) = t.text.split_once(":=") {
                    for item in lhs.split(',') {
                        out.extend(names_in(item).into_iter().take(1));
                    }
                }
            }
            Kind::While { cond, .. } if cond.text.contains("..") => {
                if let Some((var, _)) = cond.text.split_once('=') {
                    out.extend(names_in(var));
                }
            }
            _ => {}
        }
        for s in n.seqs() {
            assigned_names(s, out);
        }
    }
}

fn without_noops(nodes: Vec<Node>) -> Vec<Node> {
    nodes.into_iter().filter(|n| !n.is_noop()).collect()
}

fn joined(a: Vec<Node>, b: Vec<Node>) -> Vec<Node> {
    let all: Vec<Node> = without_noops(a)
        .into_iter()
        .chain(without_noops(b))
        .collect();
    if all.is_empty() {
        vec![Node::noop()]
    } else {
        all
    }
}

/// The merge of two decisions that follow each other, if they can be one.
fn merged(first: &Node, second: &Node) -> Option<Node> {
    let (
        Kind::If {
            cond: c1,
            yes: y1,
            no: n1,
        },
        Kind::If {
            cond: c2,
            yes: y2,
            no: n2,
        },
    ) = (&first.kind, &second.kind)
    else {
        return None;
    };
    let rel = relation(&c1.text, &c2.text)?;
    // the second one must see what the first one saw
    let mut changed = vec![];
    assigned_names(y1, &mut changed);
    assigned_names(n1, &mut changed);
    if names_in(&c1.text).iter().any(|n| changed.contains(n)) {
        return None;
    }
    let (yes, no) = match rel {
        Rel::Same => (
            joined(y1.clone(), y2.clone()),
            joined(n1.clone(), n2.clone()),
        ),
        Rel::Opposite => (
            joined(y1.clone(), n2.clone()),
            joined(n1.clone(), y2.clone()),
        ),
    };
    Some(Node {
        id: first.id,
        bg: first.bg.clone(),
        kind: Kind::If {
            cond: c1.clone(),
            yes,
            no,
        },
    })
}

/// Merges the decisions that follow each other, in `nodes` and everything inside them.
pub fn merge_ifs(nodes: &mut Vec<Node>) {
    for n in nodes.iter_mut() {
        for s in n.seqs_mut() {
            merge_ifs(s);
        }
    }
    let mut i = 0;
    while i + 1 < nodes.len() {
        match merged(&nodes[i], &nodes[i + 1]) {
            Some(mut m) => {
                // the joined branches may have decisions that follow each other now
                for s in m.seqs_mut() {
                    merge_ifs(s);
                }
                nodes[i] = m;
                nodes.remove(i + 1);
            }
            None => i += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stuki::model::fresh_id;
    use crate::stuki::rich::Rich;

    fn stmt(t: &str) -> Node {
        Node {
            id: fresh_id(),
            bg: None,
            kind: Kind::Statement(Rich::plain(t)),
        }
    }

    fn decision(cond: &str, yes: Vec<Node>, no: Vec<Node>) -> Node {
        let seq = |v: Vec<Node>| if v.is_empty() { vec![Node::noop()] } else { v };
        Node {
            id: fresh_id(),
            bg: None,
            kind: Kind::If {
                cond: Rich::plain(cond),
                yes: seq(yes),
                no: seq(no),
            },
        }
    }

    fn show(nodes: &[Node]) -> String {
        fn go(nodes: &[Node], d: usize, out: &mut String) {
            let pad = "  ".repeat(d);
            for n in nodes {
                match &n.kind {
                    Kind::Noop => {}
                    Kind::Statement(t) => out.push_str(&format!("{pad}{}\n", t.text)),
                    Kind::If { cond, yes, no } => {
                        out.push_str(&format!("{pad}[if {}]\n", cond.text));
                        go(yes, d + 1, out);
                        if no.iter().any(|n| !n.is_noop()) {
                            out.push_str(&format!("{pad}[else]\n"));
                            go(no, d + 1, out);
                        }
                    }
                    Kind::While { cond, body } => {
                        out.push_str(&format!("{pad}[while {}]\n", cond.text));
                        go(body, d + 1, out);
                    }
                    _ => out.push_str(&format!("{pad}?\n")),
                }
            }
        }
        let mut out = String::new();
        go(nodes, 0, &mut out);
        out
    }

    #[test]
    fn conditions_and_their_opposites() {
        assert!(same_cond("van", "(van)"));
        assert!(same_cond("x < y", "x<y"));
        assert!(opposite_cond("van", "nem van"));
        assert!(opposite_cond("not(found)", "found"));
        assert!(opposite_cond("not (a and b)", "(a and b)"));
        assert!(opposite_cond("x<y", "x>=y"));
        assert!(opposite_cond("x<y", "y<=x"));
        assert!(opposite_cond("x=y", "x≠y"));
        assert!(!opposite_cond("x<y", "x>y"));
        assert!(!opposite_cond("van", "nem ind"));
        assert_eq!(relation("c", "c"), Some(Rel::Same));
        assert_eq!(relation("c", "not c"), Some(Rel::Opposite));
        assert_eq!(relation("c", "d"), None);
    }

    #[test]
    fn opposite_conditions_become_if_else() {
        let mut v = vec![
            decision("found", vec![stmt("nap:=i")], vec![]),
            decision("not(found)", vec![stmt("nap:=-1")], vec![]),
        ];
        merge_ifs(&mut v);
        assert_eq!(show(&v), "[if found]\n  nap:=i\n[else]\n  nap:=-1\n");
        // the other way round: the first branch is the opposite one
        let mut v = vec![
            decision("nem van", vec![stmt("s:=-1")], vec![]),
            decision("van", vec![stmt("s:=ind")], vec![]),
        ];
        merge_ifs(&mut v);
        assert_eq!(show(&v), "[if nem van]\n  s:=-1\n[else]\n  s:=ind\n");
    }

    #[test]
    fn the_same_condition_is_one_decision_with_both_branches() {
        let mut v = vec![
            decision("van", vec![stmt("a:=1")], vec![stmt("b:=1")]),
            decision("van", vec![stmt("c:=1")], vec![stmt("d:=1")]),
            decision("van", vec![stmt("e:=1")], vec![]),
        ];
        merge_ifs(&mut v);
        assert_eq!(
            show(&v),
            "[if van]\n  a:=1\n  c:=1\n  e:=1\n[else]\n  b:=1\n  d:=1\n"
        );
    }

    #[test]
    fn not_when_the_first_decision_changes_what_the_condition_reads() {
        let mut v = vec![
            decision("found", vec![stmt("found:=false")], vec![]),
            decision("not found", vec![stmt("x:=1")], vec![]),
        ];
        merge_ifs(&mut v);
        assert_eq!(v.len(), 2);
        // an element of an array the condition reads
        let mut v = vec![
            decision("t[1]>0", vec![stmt("t[2]:=0"), stmt("t[1]:=0")], vec![]),
            decision("t[1]<=0", vec![stmt("x:=1")], vec![]),
        ];
        merge_ifs(&mut v);
        assert_eq!(v.len(), 2);
        // other conditions and other things in between
        let mut v = vec![
            decision("a", vec![stmt("x:=1")], vec![]),
            stmt("y:=2"),
            decision("not a", vec![stmt("x:=3")], vec![]),
            decision("b", vec![stmt("x:=1")], vec![]),
        ];
        merge_ifs(&mut v);
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn inside_loops_and_branches_too() {
        let inner = vec![
            decision("p", vec![stmt("a:=1")], vec![]),
            decision("not p", vec![stmt("a:=2")], vec![]),
        ];
        let mut v = vec![Node {
            id: fresh_id(),
            bg: None,
            kind: Kind::While {
                cond: Rich::plain("i=1..n"),
                body: inner,
            },
        }];
        merge_ifs(&mut v);
        assert_eq!(
            show(&v),
            "[while i=1..n]\n  [if p]\n    a:=1\n  [else]\n    a:=2\n"
        );
    }
}
