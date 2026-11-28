//! The friendly version of the console program (`IoLayout::friendly`).
//!
//! The program has both versions of Input and Output: the one the Code tab runs, which reads the
//! values unchecked and prints them plainly, and a friendly one for a person at the keyboard
//! (`Console.IsInputRedirected` / `IsOutputRedirected` choose). The friendly input asks for every
//! line, says what it wants, and asks again until the line is right: the number of values, and the
//! bounds the specification puts on them. Those come from the types (`N`, `[a..b]`) and from the
//! preconditions (`Ef:`): a condition is checked as soon as the last input it talks about is
//! typed, a `∀` over the elements of an array is checked for every element. A condition that is
//! more complicated than that is not checked (and a note says so). The friendly output says what
//! each value is.
//!
//! No `break`, `continue` or `return` is used: every retry is a `do { … } while (!ok);`.

use super::*;

/// A condition on an input: the C# code, and the specification's words for the message.
#[derive(Clone)]
pub(super) struct Cond {
    code: String,
    text: String,
}

/// What the specification demands of the inputs.
#[derive(Default)]
pub(super) struct NiceConds {
    /// By input variable: checked when it has been typed.
    scalar: HashMap<String, Vec<Cond>>,
    /// By input array: the names of the loop variables the conditions use, and the conditions
    /// (they are about the element at those indexes).
    elem: HashMap<String, (Vec<String>, Vec<Cond>)>,
}

/// What a line holds, in order.
#[derive(Clone)]
enum Seg {
    Tok {
        target: String,
        ty: Ty,
        conds: Vec<Cond>,
    },
    Loop {
        var: String,
        lo: String,
        hi: String,
        body: Vec<Seg>,
    },
}

fn split_and<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match &*e.kind {
        ExprKind::Bin(BinOp::And, a, b) => {
            split_and(a, out);
            split_and(b, out);
        }
        _ => out.push(e),
    }
}

/// An expression of plain values (no quantifier, no array): what can be checked on a scalar.
/// `elements`: array elements and fields are fine too.
fn plain(e: &Expr, elements: bool) -> bool {
    let p = |x: &Expr| plain(x, elements);
    match &*e.kind {
        ExprKind::Int(_)
        | ExprKind::Real(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::Var(_) => true,
        ExprKind::Neg(a) | ExprKind::Not(a) => p(a),
        ExprKind::Bin(_, a, b) => p(a) && p(b),
        ExprKind::Cmp(xs, _) => xs.iter().all(p),
        ExprKind::In(a, b, c) => p(a) && p(b) && p(c),
        ExprKind::Call(_, xs) => xs.iter().all(p),
        ExprKind::Index(a, xs) => elements && p(a) && xs.iter().all(p),
        ExprKind::Field(a, _) => elements && p(a),
        _ => false,
    }
}

/// The names the array `arr` is indexed with in `e` (the first place it is), if they are all
/// variables.
fn index_names(e: &Expr, arr: &str) -> Option<Vec<String>> {
    let mut found: Option<Option<Vec<String>>> = None;
    visit(e, &mut |x| {
        if found.is_none()
            && let ExprKind::Index(a, xs) = &*x.kind
            && matches!(&*a.kind, ExprKind::Var(n) if n == arr)
        {
            found = Some(
                xs.iter()
                    .map(|i| match &*i.kind {
                        ExprKind::Var(n) => Some(n.clone()),
                        _ => None,
                    })
                    .collect(),
            );
        }
    });
    found.flatten()
}

fn visit(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match &*e.kind {
        ExprKind::Neg(a) | ExprKind::Not(a) | ExprKind::Field(a, _) => visit(a, f),
        ExprKind::Bin(_, a, b) => {
            visit(a, f);
            visit(b, f);
        }
        ExprKind::Cmp(xs, _) | ExprKind::Call(_, xs) => xs.iter().for_each(|x| visit(x, f)),
        ExprKind::In(a, b, c) => {
            visit(a, f);
            visit(b, f);
            visit(c, f);
        }
        ExprKind::Index(a, xs) => {
            visit(a, f);
            xs.iter().for_each(|x| visit(x, f));
        }
        _ => {}
    }
}

/// `code` with the identifiers `from` replaced by the ones in `to` (at the same place in the
/// list; fields (`.name`) and the insides of text are left alone).
fn rename(code: &str, from: &[String], to: &[String]) -> String {
    let c: Vec<char> = code.chars().collect();
    let (mut out, mut i) = (String::new(), 0);
    while i < c.len() {
        if c[i] == '"' {
            let end = (i + 1..c.len())
                .find(|&j| c[j] == '"' && c[j - 1] != '\\')
                .map_or(c.len(), |j| j + 1);
            out.extend(&c[i..end]);
            i = end;
        } else if c[i].is_alphanumeric() || c[i] == '_' {
            let end = (i..c.len())
                .find(|&j| !(c[j].is_alphanumeric() || c[j] == '_'))
                .unwrap_or(c.len());
            let word: String = c[i..end].iter().collect();
            let field = i > 0 && c[i - 1] == '.';
            match from.iter().position(|f| *f == word) {
                Some(k) if !field && k < to.len() => out.push_str(&to[k]),
                _ => out.push_str(&word),
            }
            i = end;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// A term of a product: in parentheses when it has operators.
fn paren(s: &str) -> String {
    if s.contains(' ') {
        format!("({s})")
    } else {
        s.to_string()
    }
}

/// How many tokens the segments are, as a C# expression (a number when it is known).
fn seg_count(segs: &[Seg]) -> String {
    let mut fixed = 0usize;
    let mut terms: Vec<String> = vec![];
    for s in segs {
        match s {
            Seg::Tok { .. } => fixed += 1,
            Seg::Loop { lo, hi, body, .. } => {
                let len = if lo == "1" {
                    hi.clone()
                } else {
                    format!("{hi} - {lo} + 1")
                };
                let inner = seg_count(body);
                terms.push(if inner == "1" {
                    len
                } else {
                    format!("{} * {}", paren(&len), paren(&inner))
                });
            }
        }
    }
    if terms.is_empty() {
        return fixed.to_string();
    }
    if fixed > 0 {
        terms.push(fixed.to_string());
    }
    terms.join(" + ")
}

fn has_conds(segs: &[Seg]) -> bool {
    segs.iter().any(|s| match s {
        Seg::Tok { conds, .. } => !conds.is_empty(),
        Seg::Loop { body, .. } => has_conds(body),
    })
}

fn cond_texts(segs: &[Seg], out: &mut Vec<String>) {
    for s in segs {
        match s {
            Seg::Tok { conds, .. } => {
                for c in conds {
                    if !out.contains(&c.text) {
                        out.push(c.text.clone());
                    }
                }
            }
            Seg::Loop { body, .. } => cond_texts(body, out),
        }
    }
}

/// `body` inside the loops `bounds` (the first is the outermost).
fn nest(bounds: &[(String, String, String)], body: Vec<Seg>) -> Vec<Seg> {
    match bounds.split_first() {
        None => body,
        Some(((var, lo, hi), rest)) => vec![Seg::Loop {
            var: var.clone(),
            lo: lo.clone(),
            hi: hi.clone(),
            body: nest(rest, body),
        }],
    }
}

impl Gen<'_> {
    fn tr(&self, en: &str, hu: &str) -> String {
        if self.english { en } else { hu }.to_string()
    }

    /// What the output says before a value: `name = ` for the test values, a sentence for the
    /// friendly output.
    pub(super) fn label_text(&self, name: &str, many: bool) -> String {
        if !self.nice_out {
            return format!("{name} = ");
        }
        match (self.english, many) {
            (true, false) => format!("The value of '{name}' is: "),
            (true, true) => format!("The values of '{name}' are: "),
            (false, false) => format!("'{name}' értéke: "),
            (false, true) => format!("'{name}' értékei: "),
        }
    }

    /// `if (cond) a; else b;`
    pub(super) fn choose(&mut self, cond: &str, a: &str, b: &str) {
        self.line(&format!("if ({cond})"));
        self.depth += 1;
        self.line(a);
        self.depth -= 1;
        self.line("else");
        self.depth += 1;
        self.line(b);
        self.depth -= 1;
    }

    // ------------------------------------------------------------------ the conditions

    /// What the types and the precondition say about the inputs `ins`.
    fn nice_conds(&mut self, ins: &[&crate::parser::Decl]) -> NiceConds {
        let spec = self.spec;
        let order: HashMap<String, usize> = ins
            .iter()
            .enumerate()
            .map(|(i, d)| (d.name.clone(), i))
            .collect();
        let mut out = NiceConds::default();
        let mut conj = vec![];
        split_and(&spec.pre, &mut conj);
        for c in conj {
            if matches!(&*c.kind, ExprKind::Bool(true)) {
                continue;
            }
            if !self.place_cond(c, ins, &order, &mut out) {
                self.note(format!(
                    "the precondition `{}` is not checked when the input is typed",
                    spec.text(c)
                ));
            }
        }
        // a size must not be negative, even when the specification says nothing about it
        for d in ins {
            if !matches!(self.vars[&d.name], Ty::Arr { .. }) {
                continue;
            }
            for (_, hi) in dims_of(spec, &d.ty).0 {
                for v in hi.map(vars_in).unwrap_or_default() {
                    let Some(&k) = order.get(&v) else { continue };
                    let known = out.scalar.contains_key(&v)
                        || !self.type_conds(&ins[k].ty, &id(&v), &v).is_empty();
                    if !known && self.vars[&v] == Ty::Int {
                        out.scalar.entry(v.clone()).or_default().push(Cond {
                            code: format!("{} >= 0", id(&v)),
                            text: format!("{v} >= 0 (a size)"),
                        });
                    }
                }
            }
        }
        out
    }

    /// Puts one conjunct of the precondition where it is checked; `false`: it cannot be.
    fn place_cond(
        &mut self,
        c: &Expr,
        ins: &[&crate::parser::Decl],
        order: &HashMap<String, usize>,
        out: &mut NiceConds,
    ) -> bool {
        let spec = self.spec;
        let text = spec.text(c).to_string();
        if matches!(&*c.kind, ExprKind::Quant { all: true, .. }) {
            return self.place_forall(c, ins, order, out);
        }
        if !plain(c, false) {
            return false;
        }
        let mut last: Option<(usize, String)> = None;
        for v in vars_in(c) {
            let Some(&k) = order.get(&v) else {
                return false;
            };
            if !self.vars.get(&v).is_some_and(Ty::is_scalar) {
                return false;
            }
            if last.as_ref().is_none_or(|(l, _)| k > *l) {
                last = Some((k, v));
            }
        }
        let Some((_, target)) = last else {
            return false;
        };
        let code = self.ex(c, 0);
        out.scalar
            .entry(target)
            .or_default()
            .push(Cond { code, text });
        true
    }

    /// `∀i∈[lo..hi]: (conditions on a[i])`: a condition of the elements of the array `a`.
    fn place_forall(
        &mut self,
        c: &Expr,
        ins: &[&crate::parser::Decl],
        order: &HashMap<String, usize>,
        out: &mut NiceConds,
    ) -> bool {
        let spec = self.spec;
        let mut qs: Vec<(&String, &Expr, &Expr)> = vec![];
        let mut cur = c;
        while let ExprKind::Quant {
            all: true,
            var,
            lo,
            hi,
            body,
        } = &*cur.kind
        {
            qs.push((var, lo, hi));
            cur = body;
        }
        let mut parts = vec![];
        split_and(cur, &mut parts);
        let qnames: Vec<&String> = qs.iter().map(|q| q.0).collect();
        let mut found: Vec<(String, Vec<String>, Cond)> = vec![];
        for part in parts {
            if !plain(part, true) {
                return false;
            }
            let (mut arr, mut others) = (vec![], vec![]);
            for v in vars_in(part) {
                if qnames.contains(&&v) {
                    continue;
                }
                match self.vars.get(&v) {
                    Some(Ty::Arr { .. }) => arr.push(v),
                    Some(_) => others.push(v),
                    None => return false,
                }
            }
            let [a] = arr.as_slice() else { return false };
            let Some(&ka) = order.get(a) else {
                return false;
            };
            // the other inputs are typed before the array
            if others
                .iter()
                .any(|o| !order.get(o).is_some_and(|k| *k < ka))
            {
                return false;
            }
            let Some(names) = index_names(part, a) else {
                return false;
            };
            let Ty::Arr { dims, .. } = self.vars[a] else {
                return false;
            };
            let distinct = names
                .iter()
                .enumerate()
                .all(|(i, n)| !names[..i].contains(n));
            if names.len() != dims || !distinct || names.iter().any(|n| !qnames.contains(&n)) {
                return false;
            }
            // the quantifier may cover only some of the indexes
            let (dd, _) = dims_of(spec, &ins[ka].ty);
            let mut guard = vec![];
            for (v, lo, hi) in &qs {
                let Some(k) = names.iter().position(|n| n == *v) else {
                    return false;
                };
                let full = dd.get(k).is_some_and(|(dl, dh)| {
                    spec.text(lo) == spec.text(dl)
                        && dh.is_some_and(|dh| spec.text(hi) == spec.text(dh))
                });
                if !full {
                    let (l, h) = (self.ex(lo, 5), self.ex(hi, 5));
                    guard.push(format!("{v} >= {l} && {v} <= {h}"));
                }
            }
            let body = self.ex(part, 0);
            let code = if guard.is_empty() {
                body
            } else {
                format!("!({}) || {body}", guard.join(" && "))
            };
            found.push((
                a.clone(),
                names,
                Cond {
                    code,
                    text: spec.text(c).to_string(),
                },
            ));
        }
        for (a, names, cond) in found {
            let e = out.elem.entry(a).or_insert_with(|| (names.clone(), vec![]));
            if e.0 != names {
                // another name for the indexes: this one cannot share the loops
                return false;
            }
            e.1.push(cond);
        }
        true
    }

    /// What the type of a value demands (`N`: not negative, `[a..b]`: in the interval).
    fn type_conds(&mut self, t: &Type, target: &str, label: &str) -> Vec<Cond> {
        let spec = self.spec;
        match spec.resolve(t) {
            Some(Type::Nat) => vec![Cond {
                code: format!("{target} >= 0"),
                text: format!("{label} >= 0"),
            }],
            Some(Type::Interval(lo, hi)) => {
                let (l, h) = (self.ex(lo, 5), self.ex(hi, 5));
                vec![Cond {
                    code: format!("{target} >= {l} && {target} <= {h}"),
                    text: format!("{} <= {label} <= {}", spec.text(lo), spec.text(hi)),
                }]
            }
            _ => vec![],
        }
    }

    /// What the type of the field `f` of a record of type `t` demands.
    fn field_conds(&mut self, t: &Type, f: &str, target: &str, label: &str) -> Vec<Cond> {
        let spec = self.spec;
        let Some(Type::Record(fs)) = spec.resolve(t) else {
            return vec![];
        };
        let Some((_, ft)) = fs.iter().find(|(n, _)| n == f) else {
            return vec![];
        };
        self.type_conds(ft, target, label)
    }

    // ------------------------------------------------------------------ input

    /// The friendly input section: the same inputs as [`Gen::input_section`], typed.
    pub(super) fn input_section_nice(&mut self, ins: &[&crate::parser::Decl]) {
        let conds = self.nice_conds(ins);
        let plan = self.io_plan(ins, true);
        let sor = self.fresh("sor");
        let ok = self.fresh("ok");
        self.line(&format!("string[] {sor};"));
        self.line(&format!("bool {ok};"));
        // a value that has no fixed number of tokens, or that is a text, has lines of its own
        let fixed = |g: &Self, k: usize| {
            let ty = &g.vars[&ins[k].name];
            (ty.is_scalar() && *ty != Ty::Str)
                || (matches!(ty, Ty::Rec(_)) && plan[k].level == Level::Single)
        };
        let mut k = 0;
        while k < ins.len() {
            if fixed(self, k) {
                let mut group = vec![k];
                while k + 1 < ins.len() && plan[k + 1].join && fixed(self, k + 1) {
                    k += 1;
                    group.push(k);
                }
                self.nice_group(ins, &group, &conds, &sor, &ok);
            } else {
                self.nice_decl(ins[k], plan[k].level, &conds, &sor, &ok);
            }
            k += 1;
        }
        self.taken.remove(&sor);
        self.taken.remove(&ok);
    }

    /// `'a'`, `'a' and 'b'`, `'a', 'b' and 'c'`.
    fn name_list(&self, labels: &[String]) -> String {
        let q: Vec<String> = labels.iter().map(|l| format!("'{l}'")).collect();
        match q.split_last() {
            None => String::new(),
            Some((one, [])) => one.clone(),
            Some((last, init)) => format!("{} {} {last}", init.join(", "), self.tr("and", "és")),
        }
    }

    /// Values that share a line: numbers, truth values, characters and records of them.
    fn nice_group(
        &mut self,
        ins: &[&crate::parser::Decl],
        group: &[usize],
        conds: &NiceConds,
        sor: &str,
        ok: &str,
    ) {
        let (mut segs, mut labels, mut before) = (vec![], vec![], vec![]);
        for &k in group {
            let d = ins[k];
            let name = id(&d.name);
            match self.vars[&d.name].clone() {
                Ty::Rec(r) => {
                    before.push(format!("{name} = new {r}();"));
                    for (f, ft) in self.fields_of(&r) {
                        if ft.is_scalar() {
                            let target = format!("{name}.{}", id(&f));
                            let label = format!("{}.{f}", d.name);
                            let conds = self.field_conds(&d.ty, &f, &target, &label);
                            segs.push(Seg::Tok {
                                target,
                                ty: ft,
                                conds,
                            });
                            labels.push(label);
                        } else {
                            self.not_read_field(&f, &r);
                        }
                    }
                }
                t => {
                    if !self.statics {
                        // C# wants every path to give the value, also the ones that ask again
                        before.push(format!("{name} = {};", self.default_lit(&t)));
                    }
                    let mut cs = self.type_conds(&d.ty, &name, &d.name);
                    cs.extend(conds.scalar.get(&d.name).cloned().unwrap_or_default());
                    segs.push(Seg::Tok {
                        target: name,
                        ty: t,
                        conds: cs,
                    });
                    labels.push(d.name.clone());
                }
            }
        }
        for l in before {
            self.line(&l);
        }
        let prompt = quote(&format!(
            "{}{}: ",
            self.tr("input ", "add meg: "),
            self.name_list(&labels)
        ));
        self.nice_line(&prompt, &segs, sor, ok);
    }

    /// A text, a record with a line per field, or an array.
    fn nice_decl(
        &mut self,
        d: &crate::parser::Decl,
        level: Level,
        conds: &NiceConds,
        sor: &str,
        ok: &str,
    ) {
        let name = id(&d.name);
        match self.vars[&d.name].clone() {
            Ty::Str => {
                let p = quote(&format!("{}'{}': ", self.tr("input ", "add meg: "), d.name));
                self.line(&format!("Console.Write({p});"));
                self.line(&format!("{name} = Console.ReadLine() ?? \"\";"));
            }
            Ty::Rec(r) => {
                self.line(&format!("{name} = new {r}();"));
                for (f, ft) in self.fields_of(&r) {
                    if ft.is_scalar() {
                        let p = quote(&format!(
                            "{}'{}.{f}': ",
                            self.tr("input ", "add meg: "),
                            d.name
                        ));
                        let target = format!("{name}.{}", id(&f));
                        let conds =
                            self.field_conds(&d.ty, &f, &target, &format!("{}.{f}", d.name));
                        let seg = Seg::Tok {
                            target,
                            ty: ft,
                            conds,
                        };
                        self.nice_line(&p, &[seg], sor, ok);
                    } else {
                        self.not_read_field(&f, &r);
                    }
                }
            }
            Ty::Arr { .. } => self.nice_array(d, level, conds, sor, ok),
            _ => {}
        }
    }

    /// An array: made, then typed in the layout `level`.
    fn nice_array(
        &mut self,
        d: &crate::parser::Decl,
        level: Level,
        conds: &NiceConds,
        sor: &str,
        ok: &str,
    ) {
        let spec = self.spec;
        let Ty::Arr { dims, elem, .. } = self.vars[&d.name].clone() else {
            return;
        };
        let name = id(&d.name);
        let alloc = self.alloc(&d.ty);
        self.line(&format!("{name} = {alloc};"));
        if dims > 2 {
            self.note(format!(
                "`{}` has more than two dimensions: it is not read",
                d.name
            ));
            return;
        }
        let (dd, elem_type) = dims_of(spec, &d.ty);
        let (names, from_spec) = conds.elem.get(&d.name).cloned().unwrap_or_default();
        let mut bounds: Vec<(String, String, String)> = vec![];
        for (k, (lo, hi)) in dd.iter().enumerate() {
            let v = self.fresh(if k == 0 { "j" } else { "q" });
            let l = self.ex(lo, 0);
            let h = match hi {
                Some(h) => self.ex(h, 5),
                None => format!("{name}.Length - 1"),
            };
            bounds.push((v, l, h));
        }
        let vars: Vec<String> = bounds.iter().map(|b| b.0.clone()).collect();
        // the conditions of the specification name the indexes their own way
        let from_spec: Vec<Cond> = from_spec
            .into_iter()
            .map(|c| Cond {
                code: rename(&c.code, &names, &vars),
                text: c.text,
            })
            .collect();
        let target = format!("{name}[{}]", vars.join(", "));
        // the element as the prompt says it: `szel[{j}]`
        let shown = format!(
            "{}[{}]",
            d.name,
            vars.iter()
                .map(|v| format!("{{{v}}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let ask = self.tr("input ", "add meg: ");
        // how the messages call an element
        let any = format!("{}[*]", d.name);
        // the tokens of one element
        let mut toks: Vec<(Seg, String)> = vec![];
        match &*elem {
            Ty::Rec(r) => {
                for (f, ft) in self.fields_of(r) {
                    if ft.is_scalar() {
                        let at = format!("{target}.{}", id(&f));
                        let label = format!("{shown}.{f}");
                        let conds = self.field_conds(elem_type, &f, &at, &format!("{any}.{f}"));
                        toks.push((
                            Seg::Tok {
                                target: at,
                                ty: ft,
                                conds,
                            },
                            label,
                        ));
                    } else {
                        self.not_read_field(&f, r);
                    }
                }
                if let Some((Seg::Tok { conds, .. }, _)) = toks.last_mut() {
                    conds.extend(from_spec);
                }
            }
            e => {
                let mut cs = self.type_conds(elem_type, &target, &any);
                cs.extend(from_spec);
                toks.push((
                    Seg::Tok {
                        target: target.clone(),
                        ty: e.clone(),
                        conds: cs,
                    },
                    shown.clone(),
                ));
            }
        }
        let segs: Vec<Seg> = toks.iter().map(|t| t.0.clone()).collect();
        let fields = toks
            .iter()
            .map(|t| t.1.rsplit('.').next().unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let is_rec = matches!(*elem, Ty::Rec(_));
        let one_line = |g: &Self, what: &str, count: &str| {
            let n = if count.chars().all(|c| c.is_ascii_digit()) {
                count.to_string()
            } else if count.contains('?') {
                format!("{{({count})}}")
            } else {
                format!("{{{count}}}")
            };
            let unit = g.tr(
                "values, separated by spaces",
                "érték, szóközökkel elválasztva",
            );
            format!("$\"{ask}{what} ({n} {unit}): \"")
        };
        match (dims, level) {
            // the whole value in one line
            (_, Level::Single) => {
                let all = nest(&bounds, segs);
                let p = one_line(self, &format!("'{}'", d.name), &seg_count(&all));
                self.nice_line(&p, &all, sor, ok);
            }
            // a record per line
            (1, Level::Rows) if is_rec => {
                let (v, l, h) = bounds[0].clone();
                self.line(&format!("for (int {v} = {l}; {v} <= {h}; {v}++)"));
                self.open();
                let p = format!("$\"{ask}'{shown}' ({fields}): \"");
                self.nice_line(&p, &segs, sor, ok);
                self.close();
            }
            // a row of a table per line
            (_, Level::Rows) => {
                let (v, l, h) = bounds[0].clone();
                self.line(&format!("for (int {v} = {l}; {v} <= {h}; {v}++)"));
                self.open();
                let row = nest(&bounds[1..], segs);
                let what = self.tr("row", "sor");
                let p = one_line(
                    self,
                    &format!("{what} {{{v}}} of '{}'", d.name),
                    &seg_count(&row),
                );
                self.nice_line(&p, &row, sor, ok);
                self.close();
            }
            // every value in a line of its own
            _ => {
                for (v, l, h) in &bounds {
                    self.line(&format!("for (int {v} = {l}; {v} <= {h}; {v}++)"));
                    self.open();
                }
                for (seg, label) in toks {
                    let p = format!("$\"{ask}'{label}': \"");
                    self.nice_line(&p, &[seg], sor, ok);
                }
                for _ in &bounds {
                    self.close();
                }
            }
        }
        for v in vars {
            self.taken.remove(&v);
        }
    }

    /// One line, typed until it is right.
    fn nice_line(&mut self, prompt: &str, segs: &[Seg], sor: &str, ok: &str) {
        let count = seg_count(segs);
        let looped = segs.iter().any(|s| matches!(s, Seg::Loop { .. }));
        let mut texts = vec![];
        cond_texts(segs, &mut texts);
        self.line("do");
        self.open();
        self.line(&format!("Console.Write({prompt});"));
        self.line(&format!(
            "{sor} = (Console.ReadLine() ?? \"\").Split(' ', StringSplitOptions.RemoveEmptyEntries);"
        ));
        self.line(&format!("{ok} = {sor}.Length == {count};"));
        self.line(&format!("if (!{ok})"));
        self.open();
        let msg = if count.chars().all(|c| c.is_ascii_digit()) {
            format!(
                "\"{} {count} {}\"",
                self.tr("exactly", "pontosan"),
                self.tr("value(s) are needed", "érték kell")
            )
        } else {
            format!(
                "$\"{} {{{}}} {}\"",
                self.tr("exactly", "pontosan"),
                if count.contains('?') {
                    format!("({count})")
                } else {
                    count.clone()
                },
                self.tr("value(s) are needed", "érték kell")
            )
        };
        self.line(&format!("Console.WriteLine({msg});"));
        self.close();
        self.line("else");
        self.open();
        let p = if looped { Some(self.fresh("p")) } else { None };
        if let Some(p) = &p {
            self.line(&format!("int {p} = 0;"));
        }
        let mut at = if looped { None } else { Some(0) };
        self.parse_pass(segs, sor, ok, p.as_deref(), &mut at);
        if has_conds(segs) {
            self.line(&format!("if ({ok})"));
            self.open();
            self.cond_pass(segs, ok);
            self.close();
        }
        self.line(&format!("if (!{ok})"));
        self.open();
        let retry = self.tr("invalid input, try again", "hibás adat, próbáld újra");
        let text = if texts.is_empty() {
            quote(&retry)
        } else {
            quote(&format!(
                "{retry} ({}: {})",
                self.tr("required", "elvárt"),
                texts.join(", ")
            ))
        };
        self.line(&format!("Console.WriteLine({text});"));
        self.close();
        self.close();
        self.depth -= 1;
        self.line(&format!("}} while (!{ok});"));
        if let Some(p) = p {
            self.taken.remove(&p);
        }
    }

    fn parse_pass(
        &mut self,
        segs: &[Seg],
        sor: &str,
        ok: &str,
        p: Option<&str>,
        at: &mut Option<usize>,
    ) {
        for s in segs {
            match s {
                Seg::Tok { target, ty, .. } => {
                    let tok = match (at.as_mut(), p) {
                        (Some(i), _) => {
                            let t = format!("{sor}[{i}]");
                            *i += 1;
                            t
                        }
                        (None, Some(p)) => format!("{sor}[{p}++]"),
                        (None, None) => format!("{sor}[0]"),
                    };
                    self.parse_tok(ty, &tok, target, ok);
                }
                Seg::Loop { var, lo, hi, body } => {
                    self.line(&format!("for (int {var} = {lo}; {var} <= {hi}; {var}++)"));
                    self.open();
                    self.parse_pass(body, sor, ok, p, at);
                    self.close();
                }
            }
        }
    }

    /// Reads one token into `target`; `ok` is cleared when it is not a value of the type.
    fn parse_tok(&mut self, ty: &Ty, tok: &str, target: &str, ok: &str) {
        match ty {
            Ty::Int | Ty::Char => {
                let c = self.cs(ty);
                self.line(&format!("{ok} = {c}.TryParse({tok}, out {target}) & {ok};"));
            }
            Ty::Real => {
                self.uses_culture = true;
                self.line(&format!(
                    "{ok} = double.TryParse({tok}, NumberStyles.Float, CultureInfo.InvariantCulture, out {target}) & {ok};"
                ));
            }
            Ty::Bool => {
                let w = self.fresh("s");
                self.line(&format!("string {w} = {tok};"));
                let valid = ["igaz", "true", "True", "hamis", "false", "False"]
                    .iter()
                    .map(|v| format!("{w} == \"{v}\""))
                    .collect::<Vec<_>>()
                    .join(" || ");
                self.line(&format!("{ok} = ({valid}) & {ok};"));
                let v = self.parse(ty, &w);
                self.line(&format!("{target} = {v};"));
                self.taken.remove(&w);
            }
            _ => self.line(&format!("{target} = {tok};")),
        }
    }

    /// The bounds, once the line is read: for every value that has some.
    fn cond_pass(&mut self, segs: &[Seg], ok: &str) {
        for s in segs {
            match s {
                Seg::Tok { conds, .. } if !conds.is_empty() => {
                    let parts: Vec<String> = conds
                        .iter()
                        .map(|c| {
                            if conds.len() > 1 {
                                format!("({})", c.code)
                            } else {
                                c.code.clone()
                            }
                        })
                        .collect();
                    self.line(&format!("{ok} = {ok} && {};", parts.join(" && ")));
                }
                Seg::Loop { var, lo, hi, body } if has_conds(body) => {
                    self.line(&format!("for (int {var} = {lo}; {var} <= {hi}; {var}++)"));
                    self.open();
                    self.cond_pass(body, ok);
                    self.close();
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------ output

    /// The friendly output section: every output says what it is.
    pub(super) fn output_section_nice(&mut self, outs: &[&crate::parser::Decl]) {
        let (labels, nice) = (self.labels, self.nice_out);
        self.labels = true;
        self.nice_out = true;
        self.output_section(outs, &None);
        self.labels = labels;
        self.nice_out = nice;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::stuki::from_spec::from_spec;

    /// Two numbers that can share a line, bounds, an array with a bound of its elements.
    const SZEL: &str = "Be:\nn∈N,\nk∈N,\nszel∈Z[1..n]\nKi:\nnap∈N\nEf:\n10<=n és n<=100 és 10<=k<=100 és ∀i∈[1..n]:(szel[i]>=0)\nUf:\nnap=DARAB(i=1..n, szel[i]>=k)\n";
    /// A table, a real, a truth value, a character and a text.
    const MATS: &str = "In:\nn∈N,\nm∈N,\nt∈Z[1..n,1..m],\nx∈R,\nb∈L,\nc∈C,\nname∈S\nOut:\ns∈Z,\nr∈R\nPre:\n1<=n és n<=3 és 1<=m és m<=3 és 0<=x és x<=1 és ∀i∈[1..n]:(∀j∈[1..m]:(t[i,j]>=0))\nPost:\ns=SUM(i=1..n, SUM(j=1..m, t[i,j])) és r=x és (b->s>=0) és c=c és name=name\n";
    /// Records, with a bound in the type of a field.
    const RECS: &str = "In:\nn∈N,\nhallgatok∈Hallgato[1..n],\nHallgato=(nev:S x jegy:[1..5])\nOut:\ndb∈N\nPre:\n1<=n és n<=10\nPost:\ndb=COUNT(i=1..n, hallgatok[i].jegy=5)\n";
    /// A specification with a function (its variables are fields), chained bounds.
    const WIND: &str = "In: n∈Z, k∈N, szel∈N[1..n]\nAux: found∈L\nOut: nap∈Z\nFn: nicewind: N->L, nicewind(x) = FORALL(i=1..k, 10≤szel[i+x-1]≤100)\nPre: 1≤n≤1000 and 1≤k≤10 and ∀i∈[1..n]:(0≤szel[i]≤200)\nPost: (found, nap) = SEARCH(i=1..n-(k-1), nicewind(i)) and not(found) -> nap = -1\n";
    const ALL: [(&str, &str); 4] = [
        ("szel", SZEL),
        ("mats", MATS),
        ("recs", RECS),
        ("wind", WIND),
    ];

    fn friendly(joined: bool, level: Level, spec: &Spec) -> IoLayout {
        let mut l = IoLayout::default().with_friendly(true);
        for d in spec.decls.iter().filter(|d| d.role == Role::In) {
            l.set_level(&d.name, level);
            l.set_join(&d.name, joined);
        }
        l
    }

    fn code(spec_text: &str, functions: bool, layout: &IoLayout) -> Output {
        let spec = parse(spec_text).unwrap();
        let program = from_spec(&spec, "t").program;
        generate_opts(&spec, &program, Inputs::Console, "", layout, functions).unwrap()
    }

    #[test]
    fn nothing_changes_unless_asked_for() {
        for (name, text) in ALL {
            let c = code(text, false, &IoLayout::default()).code;
            assert!(
                !c.contains("Redirected") && !c.contains("TryParse"),
                "{name}\n{c}"
            );
        }
    }

    #[test]
    fn both_versions_are_in_the_program() {
        let spec = parse(SZEL).unwrap();
        let mut l = friendly(false, Level::Auto, &spec);
        l.set_join("k", true);
        let mut p = l.clone();
        p.friendly = false;
        let plain = code(SZEL, false, &p).code;
        let c = code(SZEL, false, &l);
        // the version for redirected input is the one without the option
        let plain_lines: Vec<&str> = plain.lines().map(str::trim).collect();
        for want in ["n = int.Parse(sor[0]);", "k = int.Parse(sor[1]);"] {
            assert!(c.code.contains(want), "{}", c.code);
            assert!(plain_lines.contains(&want));
        }
        for want in [
            "if (Console.IsInputRedirected)",
            "if (Console.IsOutputRedirected)",
            "add meg: 'n' és 'k': ",
            "pontosan 2 érték kell",
            "ok = int.TryParse(sor[0], out n) & ok;",
            "ok = ok && szel[j] >= 0;",
            "elvárt:",
            "'nap' értéke: ",
        ] {
            assert!(c.code.contains(want), "{want}\n{}", c.code);
        }
        // the bounds come from the specification
        assert!(
            c.code.contains("(10 <= n)") && c.code.contains("(n <= 100)"),
            "{}",
            c.code
        );
        assert!(c.code.contains("10 <= k && k <= 100"), "{}", c.code);
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        // an English specification talks English
        let en = code(
            MATS,
            false,
            &friendly(false, Level::Auto, &parse(MATS).unwrap()),
        )
        .code;
        assert!(
            en.contains("input row {j") && en.contains("The value of 's' is: "),
            "{en}"
        );
        assert!(
            en.contains("value(s) are needed") && en.contains("invalid input, try again"),
            "{en}"
        );
    }

    /// The new lines never leave a loop or a method early.
    #[test]
    fn no_control_flow_exits() {
        for (name, text) in ALL {
            let spec = parse(text).unwrap();
            for functions in [false, true] {
                let plain = code(text, functions, &IoLayout::default()).code;
                let old: HashSet<&str> = plain.lines().map(str::trim).collect();
                let c = code(text, functions, &friendly(false, Level::Auto, &spec)).code;
                for l in c.lines().map(str::trim).filter(|l| !old.contains(l)) {
                    for bad in ["break", "continue", "return", "goto"] {
                        assert!(
                            !l.split(|c: char| !c.is_alphanumeric()).any(|w| w == bad),
                            "{name}: {l}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_sections_as_functions_have_friendly_twins() {
        let spec = parse(SZEL).unwrap();
        let c = code(SZEL, true, &friendly(false, Level::Auto, &spec)).code;
        for want in [
            "static void Input(",
            "static void Input_nice(",
            "static void Output(",
            "static void Output_nice(",
            "if (Console.IsInputRedirected)\n            Input(out n, out k, out szel);\n        else\n            Input_nice(out n, out k, out szel);",
            "if (Console.IsOutputRedirected)\n            Output(nap);\n        else\n            Output_nice(nap);",
        ] {
            assert!(c.contains(want), "{want}\n{c}");
        }
    }

    #[test]
    fn what_cannot_be_checked_is_said() {
        let text = SZEL.replace("∀i∈[1..n]:(szel[i]>=0)", "∃i∈[1..n]:(szel[i]>0)");
        let spec = parse(&text).unwrap();
        let c = code(&text, false, &friendly(false, Level::Auto, &spec));
        assert!(
            c.notes.iter().any(|n| n.contains("is not checked")),
            "{:?}",
            c.notes
        );
    }

    #[test]
    fn identifiers_are_renamed_whole() {
        let from = ["i".to_string(), "j".to_string()];
        let to = ["j".to_string(), "q".to_string()];
        assert_eq!(
            rename("t[i, j] >= x.i && id < \"i\"", &from, &to),
            "t[j, q] >= x.i && id < \"i\""
        );
        let tok = || Seg::Tok {
            target: "a".into(),
            ty: Ty::Int,
            conds: vec![],
        };
        let bounds = [("j".to_string(), "1".to_string(), "n".to_string())];
        assert_eq!(seg_count(&nest(&bounds, vec![tok()])), "n");
        assert_eq!(seg_count(&nest(&bounds, vec![tok(), tok()])), "n * 2");
        let rows = [("j".to_string(), "0".to_string(), "n".to_string())];
        assert_eq!(seg_count(&nest(&rows, vec![tok()])), "n - 0 + 1");
    }

    /// Every combination compiles (where there is a C# compiler to try it).
    #[test]
    fn all_layouts_compile() {
        if std::process::Command::new("mcs")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("speci_nice_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut k = 0;
        for (name, text) in ALL {
            let spec = parse(text).unwrap();
            for level in [Level::Auto, Level::Single, Level::Rows, Level::Each] {
                for joined in [false, true] {
                    for functions in [false, true] {
                        let c = code(text, functions, &friendly(joined, level, &spec)).code;
                        let (src, exe) =
                            (dir.join(format!("p{k}.cs")), dir.join(format!("p{k}.exe")));
                        std::fs::write(&src, &c).unwrap();
                        let out = std::process::Command::new("mcs")
                            .arg(format!("-out:{}", exe.display()))
                            .arg(&src)
                            .output()
                            .unwrap();
                        assert!(
                            out.status.success(),
                            "{name} {level:?} joined={joined} functions={functions}\n{}\n{c}",
                            String::from_utf8_lossy(&out.stdout)
                        );
                        k += 1;
                    }
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
