//! Specification + structograms → a C# console program, written the way SKILL.md section 7
//! describes: one structogram element becomes one statement, arrays keep the specification's
//! indexes (`int[] t = new int[n + 1];`, index 0 unused), no LINQ in the algorithm.
//!
//! The inputs are either the values of one test case, written into the program (`Inputs::Test`),
//! or read from the console (`Inputs::Console`, see [`console_input`] for the text it reads).
//! The outputs are always printed. The program has no error handling and checks no input.

use crate::data;
use crate::eval::{Env, Value};
use crate::parser::{BinOp, Expr, ExprKind, Fragment, Role, Spec, Type, parse_fragment, vars_in};
use crate::stuki::merge::{Rel, relation};
use crate::stuki::model::{Diagram, Kind, Node, Program};
use crate::stuki::run::header;
use std::collections::{HashMap, HashSet};

/// Where the program gets its input.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Inputs {
    /// The values of one test case, written into the program.
    #[default]
    Test,
    /// Read from the console (and the outputs are printed as plain values).
    Console,
}

pub struct Output {
    pub code: String,
    /// Things that could not be translated or were guessed.
    pub notes: Vec<String>,
}

/// The C# program for `spec` and `program`. `test` is the test case text (YAML) used by
/// [`Inputs::Test`].
pub fn generate(
    spec: &Spec,
    program: &Program,
    inputs: Inputs,
    test: &str,
) -> Result<Output, String> {
    let env = match inputs {
        Inputs::Test => Some(data::load(spec, test).map_err(|d| d.title)?),
        Inputs::Console => None,
    };
    Gen::new(spec).run(program, inputs, env)
}

/// What the program of [`Inputs::Console`] reads for the test case `test`: one line per scalar,
/// one line per array (a row per line for two-dimensional ones), a record's fields in one line.
pub fn console_input(spec: &Spec, test: &str) -> Result<String, String> {
    let env = data::load(spec, test).map_err(|d| d.title)?;
    let mut g = Gen::new(spec);
    let mut out = String::new();
    for d in spec.decls.iter().filter(|d| d.role == Role::In) {
        let ty = g.ty_of(&d.ty);
        match env.get(&d.name) {
            Some(v) => g.input_lines(v, &ty, &mut out),
            None => return Err(format!("The test case has no value for `{}`", d.name)),
        }
    }
    Ok(out)
}

/// What the program of [`Inputs::Console`] prints for the test case `test`: the outputs in
/// declaration order, one value per line, an array's elements each followed by a space.
pub fn console_output(spec: &Spec, test: &str) -> Result<String, String> {
    let env = data::load(spec, test).map_err(|d| d.title)?;
    let mut g = Gen::new(spec);
    let mut out = String::new();
    for d in spec.decls.iter().filter(|d| d.role == Role::Out) {
        let ty = g.ty_of(&d.ty);
        match env.get(&d.name) {
            Some(v) => g.output_lines(v, &ty, &mut out),
            None => return Err(format!("The test case has no value for `{}`", d.name)),
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------ the Code tab

/// A structogram program with nothing drawn in it (a fresh Stuki tab has one empty diagram).
pub fn is_blank(program: &Program) -> bool {
    program.diagrams.iter().all(|d| {
        d.body.iter().all(|n| match &n.kind {
            Kind::Noop => true,
            Kind::Statement(t) => matches!(t.text.trim(), "" | "-"),
            _ => false,
        })
    })
}

/// A converted program.
pub struct Converted {
    pub code: String,
    /// Things that could not be translated or were guessed.
    pub notes: Vec<String>,
    /// Console mode: what the program reads for the test case.
    pub stdin: Option<String>,
    /// The Stuki tab had nothing drawn, so `from_spec`'s structograms were used.
    pub own_structograms: bool,
}

pub enum ConvertError {
    /// The specification doesn't parse: ` (line 3: …)`.
    Spec(String),
    /// The test case can't be used.
    Test(String),
}

/// The program of a project: from the specification and its structograms (or, when nothing is
/// drawn, the ones `from_spec` makes), for the test case `test`.
pub fn convert(
    spec_text: &str,
    title: &str,
    stuki: &Program,
    test: &str,
    inputs: Inputs,
) -> Result<Converted, ConvertError> {
    let spec = crate::parser::parse(spec_text).map_err(|d| {
        ConvertError::Spec(match d.first() {
            Some(d) if d.line > 0 => format!(" (line {}: {})", d.line, d.title),
            Some(d) => format!(" ({})", d.title),
            None => String::new(),
        })
    })?;
    let own_structograms = is_blank(stuki);
    let program = if own_structograms {
        crate::stuki::from_spec::from_spec(&spec, title).program
    } else {
        stuki.clone()
    };
    let out = generate(&spec, &program, inputs, test).map_err(ConvertError::Test)?;
    let mut notes = out.notes;
    let stdin = match inputs {
        Inputs::Console => match console_input(&spec, test) {
            Ok(t) => Some(t),
            Err(e) => {
                notes.push(format!("no input from the test case: {e}"));
                None
            }
        },
        Inputs::Test => None,
    };
    Ok(Converted {
        code: out.code,
        notes,
        stdin,
        own_structograms,
    })
}

/// Is `actual` what the test case expects? Trailing spaces and empty lines at the end do not
/// matter (an array is printed with a space after each element).
pub fn same_output(expected: &str, actual: &str) -> bool {
    let norm = |t: &str| {
        t.lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    };
    norm(expected) == norm(actual)
}

// ------------------------------------------------------------------------------ carbon

/// Percent-encoding like JavaScript's `encodeURIComponent` (`strict`: `!*'()` too).
fn percent(s: &str, strict: bool) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        let plain = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~')
            || (!strict && matches!(b, b'!' | b'*' | b'\'' | b'(' | b')'));
        if plain {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A link that opens `code` in carbon.now.sh, as an image of the program with the settings
/// the app uses (Seti theme, Hack font, C# highlighting). Carbon takes the code encoded twice.
pub fn carbon_link(code: &str) -> String {
    const SETTINGS: &str = "bg=rgba%28171%2C+184%2C+195%2C+1%29&t=seti&wt=none&l=text%2Fx-csharp&width=680&ds=true&dsyoff=20px&dsblur=68px&wc=true&wa=true&pv=56px&ph=56px&ln=false&fl=1&fm=Hack&fs=14px&lh=133%25&si=false&es=2x&wm=false";
    format!(
        "https://carbon.now.sh/?{SETTINGS}&code={}",
        percent(&percent(code, false), true)
    )
}

// ------------------------------------------------------------------------------ types

#[derive(Clone, Debug, PartialEq)]
enum Ty {
    Int,
    Real,
    Bool,
    Str,
    Char,
    Arr {
        dims: usize,
        elem: Box<Ty>,
        /// The first index is 0 (otherwise the array is one longer, index 0 unused).
        zero: bool,
    },
    Rec(String),
}

impl Ty {
    fn is_scalar(&self) -> bool {
        !matches!(self, Ty::Arr { .. } | Ty::Rec(_))
    }
}

type Fields = Vec<(String, Ty)>;

/// An ir of the structogram, parsed once so that the variables can be found before writing.
enum Ir {
    Assign(Expr, Expr),
    If(Expr, Vec<Ir>, Vec<Ir>),
    Count(String, Expr, Expr, Vec<Ir>),
    While(Expr, Vec<Ir>),
    Repeat(Expr, Vec<Ir>),
    Switch(Vec<(Option<Expr>, Vec<Ir>)>),
    Todo(String),
}

/// A variable a routine declares.
struct Var<'d> {
    name: String,
    ty: Ty,
    /// The specification's declaration (it has the array's bounds), if it is one.
    decl: Option<&'d crate::parser::Decl>,
}

struct Gen<'a> {
    spec: &'a Spec,
    /// The specification followed by the structogram texts (what the parser's offsets refer to).
    src: String,
    vars: HashMap<String, Ty>,
    /// The specification's variables: what every function starts from.
    base_vars: HashMap<String, Ty>,
    structs: Vec<(String, Fields)>,
    notes: Vec<String>,
    uses_linq: bool,
    uses_culture: bool,
    uses_fact: bool,
    out: String,
    depth: usize,
    /// The specification's variables are fields of `Program` (it has functions).
    statics: bool,
    anon: usize,
    taken: HashSet<String>,
    /// Size of arrays whose length is not known when they are made.
    fallback: String,
    /// Test mode prints `name = value`; console mode prints the values only.
    labels: bool,
    english: bool,
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];

fn id(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("@{name}")
    } else {
        name.to_string()
    }
}

fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn char_lit(s: &str) -> String {
    let c = s.chars().next().unwrap_or('\0');
    match c {
        '\'' => "'\\''".into(),
        '\\' => "'\\\\'".into(),
        '\n' => "'\\n'".into(),
        '\t' => "'\\t'".into(),
        '\0' => "'\\0'".into(),
        c => format!("'{c}'"),
    }
}

fn real(f: f64) -> String {
    let s = format!("{f}");
    if s.contains(['.', 'e', 'E', 'N', 'i']) {
        s
    } else {
        format!("{s}.0")
    }
}

fn is_io(text: &str) -> bool {
    let t = text.trim_start();
    ["Be:", "In:", "Ki:", "Out:"]
        .iter()
        .any(|k| t.starts_with(k))
}

fn flat_index(e: &Expr) -> (&Expr, Vec<&Expr>) {
    if let ExprKind::Index(b, idx) = &*e.kind {
        let (base, mut v) = flat_index(b);
        v.extend(idx.iter());
        (base, v)
    } else {
        (e, vec![])
    }
}

fn target_names(e: &Expr, out: &mut Vec<String>) {
    match &*e.kind {
        ExprKind::Var(n) => out.push(n.clone()),
        ExprKind::Index(b, _) | ExprKind::Field(b, _) => target_names(b, out),
        ExprKind::Tuple(slots) => slots.iter().flatten().for_each(|s| target_names(s, out)),
        _ => {}
    }
}

/// `(lo, hi)` per dimension and the element type of an array type.
fn dims_of<'t>(spec: &'t Spec, t: &'t Type) -> (Vec<(&'t Expr, Option<&'t Expr>)>, &'t Type) {
    let mut dims = vec![];
    let mut cur = spec.resolve(t).unwrap_or(t);
    while let Type::Array { lo, hi, elem } = cur {
        dims.push((lo, hi.as_ref()));
        cur = spec.resolve(elem).unwrap_or(elem);
    }
    (dims, cur)
}

impl<'a> Gen<'a> {
    fn new(spec: &'a Spec) -> Self {
        let mut g = Gen {
            spec,
            src: format!("{}\n", spec.src),
            vars: HashMap::new(),
            base_vars: HashMap::new(),
            structs: vec![],
            notes: vec![],
            uses_linq: false,
            uses_culture: false,
            uses_fact: false,
            out: String::new(),
            depth: 0,
            statics: false,
            anon: 0,
            taken: HashSet::new(),
            fallback: "1001".into(),
            labels: false,
            english: spec.post_kw != "Uf:",
        };
        for d in &spec.decls {
            let ty = g.ty_of(&d.ty);
            g.vars.insert(d.name.clone(), ty);
            g.taken.insert(d.name.clone());
        }
        g.base_vars = g.vars.clone();
        g
    }

    fn note(&mut self, s: impl Into<String>) {
        let s = s.into();
        if !self.notes.contains(&s) {
            self.notes.push(s);
        }
    }

    fn fresh(&mut self, base: &str) -> String {
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    base.to_string()
                } else {
                    format!("{base}{n}")
                }
            })
            .find(|n| !self.taken.contains(n))
            .unwrap();
        self.taken.insert(name.clone());
        name
    }

    // ------------------------------------------------------------------ types

    fn ty_of(&mut self, t: &Type) -> Ty {
        let spec = self.spec;
        match t {
            Type::Nat | Type::Int | Type::Interval(..) => Ty::Int,
            Type::Real => Ty::Real,
            Type::Bool => Ty::Bool,
            Type::Str => Ty::Str,
            Type::Char => Ty::Char,
            Type::Named(n) => match spec.resolve(t) {
                Some(Type::Record(fs)) => {
                    self.register(n, fs);
                    Ty::Rec(n.clone())
                }
                Some(r) => self.ty_of(r),
                None => Ty::Int,
            },
            Type::Record(fs) => {
                self.anon += 1;
                let name = format!("Rekord{}", self.anon);
                self.register(&name, fs);
                Ty::Rec(name)
            }
            Type::Array { lo, .. } => {
                let (dims, elem) = dims_of(spec, t);
                Ty::Arr {
                    dims: dims.len(),
                    elem: Box::new(self.ty_of(elem)),
                    zero: matches!(&*lo.kind, ExprKind::Int(0)),
                }
            }
        }
    }

    fn register(&mut self, name: &str, fields: &[(String, Type)]) {
        if self.structs.iter().any(|(n, _)| n == name) {
            return;
        }
        self.structs.push((name.to_string(), vec![]));
        let fs: Fields = fields
            .iter()
            .map(|(n, t)| (n.clone(), self.ty_of(t)))
            .collect();
        if let Some(s) = self.structs.iter_mut().find(|(n, _)| n == name) {
            s.1 = fs;
        }
    }

    fn cs(&self, t: &Ty) -> String {
        match t {
            Ty::Int => "int".into(),
            Ty::Real => "double".into(),
            Ty::Bool => "bool".into(),
            Ty::Str => "string".into(),
            Ty::Char => "char".into(),
            Ty::Rec(n) => n.clone(),
            Ty::Arr { dims, elem, .. } => {
                format!("{}[{}]", self.cs(elem), ",".repeat(dims.saturating_sub(1)))
            }
        }
    }

    fn default_lit(&self, t: &Ty) -> String {
        match t {
            Ty::Int => "0".into(),
            Ty::Real => "0.0".into(),
            Ty::Bool => "false".into(),
            Ty::Str => "\"\"".into(),
            Ty::Char => "'\\0'".into(),
            _ => "default".into(),
        }
    }

    fn fields_of(&self, name: &str) -> Fields {
        self.structs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, f)| f.clone())
            .unwrap_or_default()
    }

    /// The type a word of a structogram's variable list names.
    fn word_ty(&mut self, w: &str) -> Option<Ty> {
        Some(match w.trim().to_lowercase().as_str() {
            "egész" | "integer" | "int" | "z" | "n" => Ty::Int,
            "valós" | "real" | "r" => Ty::Real,
            "logikai" | "boolean" | "logical" | "l" => Ty::Bool,
            "szöveg" | "string" | "s" => Ty::Str,
            "karakter" | "character" | "char" | "c" => Ty::Char,
            "tömb" | "array" => Ty::Arr {
                dims: 1,
                elem: Box::new(Ty::Int),
                zero: false,
            },
            _ => return None,
        })
    }

    // ------------------------------------------------------------------ expressions

    /// The (approximate) type of an expression: what `/` and character literals need.
    fn kind(&mut self, e: &Expr) -> Ty {
        match &*e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Real(_) => Ty::Real,
            ExprKind::Str(_) => Ty::Str,
            ExprKind::Bool(_)
            | ExprKind::Not(_)
            | ExprKind::Cmp(..)
            | ExprKind::In(..)
            | ExprKind::Quant { .. } => Ty::Bool,
            ExprKind::Var(n) => self.vars.get(n).cloned().unwrap_or(Ty::Int),
            ExprKind::Neg(a) => self.kind(a),
            ExprKind::Bin(BinOp::And | BinOp::Or | BinOp::Imp, ..) => Ty::Bool,
            ExprKind::Bin(BinOp::Div, ..) => Ty::Real,
            ExprKind::Bin(_, a, b) => {
                if self.kind(a) == Ty::Real || self.kind(b) == Ty::Real {
                    Ty::Real
                } else {
                    Ty::Int
                }
            }
            ExprKind::Index(..) => {
                let (base, _) = flat_index(e);
                match self.kind(base) {
                    Ty::Arr { elem, .. } => *elem,
                    _ => Ty::Int,
                }
            }
            ExprKind::Field(a, f) => match self.kind(a) {
                Ty::Rec(n) => self
                    .fields_of(&n)
                    .into_iter()
                    .find(|(x, _)| x == f)
                    .map(|(_, t)| t)
                    .unwrap_or(Ty::Int),
                _ => Ty::Int,
            },
            ExprKind::Call(name, args) => match name.as_str() {
                "sqrt" => Ty::Real,
                "abs" => args.first().map_or(Ty::Int, |a| self.kind(a)),
                _ => {
                    let cod = self
                        .spec
                        .funcs
                        .get(name)
                        .and_then(|f| f.cod.first().cloned());
                    cod.map_or(Ty::Int, |t| self.ty_of(&t))
                }
            },
            ExprKind::Cases(arms, other) => arms
                .first()
                .map(|(v, _)| v)
                .or(other.as_ref())
                .map_or(Ty::Int, |v| self.kind(v)),
            ExprKind::Array(items, _) => {
                let elem = items.first().map_or(Ty::Int, |i| self.kind(i));
                Ty::Arr {
                    dims: 1,
                    elem: Box::new(elem),
                    zero: false,
                }
            }
            ExprKind::Macro { name, .. } => match name.as_str() {
                "COUNT" | "DARAB" => Ty::Int,
                "EXISTS" | "FORALL" => Ty::Bool,
                _ => Ty::Int,
            },
            ExprKind::Tuple(_) => Ty::Int,
        }
    }

    /// `e` with the precedence `min` (parenthesised when it binds weaker).
    fn ex(&mut self, e: &Expr, min: u8) -> String {
        let (s, p) = self.ex_p(e);
        if p < min { format!("({s})") } else { s }
    }

    /// `e` where a value of type `want` is needed (a one-letter string becomes a `char`).
    fn ex_want(&mut self, e: &Expr, min: u8, want: &Ty) -> String {
        if *want == Ty::Char
            && let ExprKind::Str(s) = &*e.kind
            && s.chars().count() == 1
        {
            return char_lit(s);
        }
        self.ex(e, min)
    }

    fn ex_p(&mut self, e: &Expr) -> (String, u8) {
        const OR: u8 = 1;
        const AND: u8 = 2;
        const EQ: u8 = 3;
        const REL: u8 = 4;
        const ADD: u8 = 5;
        const MUL: u8 = 6;
        const UN: u8 = 7;
        const PRIM: u8 = 8;
        match &*e.kind {
            ExprKind::Int(i) => (i.to_string(), PRIM),
            ExprKind::Real(f) => (real(*f), PRIM),
            ExprKind::Str(s) => (quote(s), PRIM),
            ExprKind::Bool(b) => (b.to_string(), PRIM),
            ExprKind::Var(n) => (id(n), PRIM),
            ExprKind::Tuple(slots) => {
                let parts: Vec<String> = slots
                    .iter()
                    .map(|s| s.as_ref().map_or("_".to_string(), |x| self.ex(x, 0)))
                    .collect();
                (format!("({})", parts.join(", ")), PRIM)
            }
            ExprKind::Array(items, _) => {
                let parts: Vec<String> = items.iter().map(|i| self.ex(i, 0)).collect();
                (format!("new[] {{ {} }}", parts.join(", ")), PRIM)
            }
            ExprKind::Neg(a) => (format!("-{}", self.ex(a, UN)), UN),
            ExprKind::Not(a) => (format!("!{}", self.ex(a, UN)), UN),
            ExprKind::Bin(op, a, b) => match op {
                BinOp::Or => (format!("{} || {}", self.ex(a, OR), self.ex(b, OR + 1)), OR),
                BinOp::And => (
                    format!("{} && {}", self.ex(a, AND), self.ex(b, AND + 1)),
                    AND,
                ),
                BinOp::Imp => (format!("!{} || {}", self.ex(a, UN), self.ex(b, OR + 1)), OR),
                BinOp::Add => (
                    format!("{} + {}", self.ex(a, ADD), self.ex(b, ADD + 1)),
                    ADD,
                ),
                BinOp::Sub => (
                    format!("{} - {}", self.ex(a, ADD), self.ex(b, ADD + 1)),
                    ADD,
                ),
                BinOp::Mul => (
                    format!("{} * {}", self.ex(a, MUL), self.ex(b, MUL + 1)),
                    MUL,
                ),
                BinOp::Mod => (
                    format!("{} % {}", self.ex(a, MUL), self.ex(b, MUL + 1)),
                    MUL,
                ),
                BinOp::IDiv => {
                    self.note("`div` became `/`: for negative numbers C# rounds toward zero, `div` rounds down");
                    (
                        format!("{} / {}", self.ex(a, MUL), self.ex(b, MUL + 1)),
                        MUL,
                    )
                }
                BinOp::Div => {
                    let real = self.kind(a) == Ty::Real || self.kind(b) == Ty::Real;
                    let l = self.ex(a, if real { MUL } else { UN });
                    let r = self.ex(b, MUL + 1);
                    if real {
                        (format!("{l} / {r}"), MUL)
                    } else {
                        (format!("(double){l} / {r}"), MUL)
                    }
                }
            },
            ExprKind::Cmp(ops_e, ops) => {
                let kinds: Vec<Ty> = ops_e.iter().map(|o| self.kind(o)).collect();
                let want = if kinds.contains(&Ty::Char) {
                    Ty::Char
                } else {
                    Ty::Int
                };
                let mut parts = vec![];
                for (k, op) in ops.iter().enumerate() {
                    let (a, b) = (&ops_e[k], &ops_e[k + 1]);
                    let (x, y) = (
                        self.ex_want(a, REL + 1, &want),
                        self.ex_want(b, REL + 1, &want),
                    );
                    parts.push(match *op {
                        "=" => format!("{x} == {y}"),
                        "!=" => format!("{x} != {y}"),
                        "|" => format!("{} % {} == 0", self.ex(b, MUL), self.ex(a, MUL + 1)),
                        "∤" => format!("{} % {} != 0", self.ex(b, MUL), self.ex(a, MUL + 1)),
                        o => format!("{x} {o} {y}"),
                    });
                }
                let prec = if parts.len() > 1 {
                    AND
                } else if matches!(ops.first(), Some(&("=" | "!=" | "|" | "∤"))) {
                    EQ
                } else {
                    REL
                };
                (parts.join(" && "), prec)
            }
            ExprKind::In(x, lo, hi) => {
                let (x1, x2) = (self.ex(x, REL + 1), self.ex(x, REL + 1));
                (
                    format!(
                        "{} <= {x1} && {x2} <= {}",
                        self.ex(lo, REL + 1),
                        self.ex(hi, REL + 1)
                    ),
                    AND,
                )
            }
            ExprKind::Index(..) => {
                let (base, idx) = flat_index(e);
                let b = self.ex(base, PRIM);
                let i: Vec<String> = idx.iter().map(|x| self.ex(x, 0)).collect();
                (format!("{b}[{}]", i.join(", ")), PRIM)
            }
            ExprKind::Field(a, f) => (format!("{}.{}", self.ex(a, PRIM), id(f)), PRIM),
            ExprKind::Call(name, args) => self.call(name, args),
            ExprKind::Quant {
                all,
                var,
                lo,
                hi,
                body,
            } => {
                self.uses_linq = true;
                self.note("an expression with ∀/∃ became a LINQ call");
                let range = self.range(lo, hi);
                let v = id(var);
                let b = self.ex(body, 0);
                let f = if *all { "All" } else { "Any" };
                (format!("{range}.{f}({v} => {b})"), PRIM)
            }
            ExprKind::Macro {
                name,
                var,
                lo,
                hi,
                args,
            } => self.macro_call(name, var, lo, hi.as_ref(), args),
            ExprKind::Cases(arms, other) => {
                let mut s = String::new();
                for (v, c) in arms {
                    s.push_str(&format!("{} ? {} : ", self.ex(c, OR), self.ex(v, OR)));
                }
                match other {
                    Some(o) => s.push_str(&self.ex(o, OR)),
                    None => s.push_str("default"),
                }
                (s, 0)
            }
        }
    }

    fn range(&mut self, lo: &Expr, hi: &Expr) -> String {
        let l = self.ex(lo, 5);
        let h = self.ex(hi, 5);
        format!("Enumerable.Range({l}, {h} - {l} + 1)")
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> (String, u8) {
        let a0 = args.first();
        match (name, a0) {
            ("abs", Some(a)) => (format!("Math.Abs({})", self.ex(a, 0)), 8),
            ("sqrt", Some(a)) => (format!("Math.Sqrt({})", self.ex(a, 0)), 8),
            ("round" | "kerekít", Some(a)) => (format!("(int)Math.Round({})", self.ex(a, 0)), 7),
            ("!", Some(a)) => {
                self.uses_fact = true;
                (format!("Factorial({})", self.ex(a, 0)), 8)
            }
            ("length" | "hossz" | "to" | "ig", Some(a)) => {
                let arr = self.ex(a, 8);
                match self.kind(a) {
                    Ty::Arr { dims, zero, .. } if dims > 1 => (
                        format!("{arr}.GetLength(0){}", if zero { "" } else { " - 1" }),
                        5,
                    ),
                    Ty::Arr { zero: true, .. } => (format!("{arr}.Length"), 8),
                    _ => (format!("{arr}.Length - 1"), 5),
                }
            }
            ("from" | "tól", Some(a)) => match self.kind(a) {
                Ty::Arr { zero: true, .. } => ("0".into(), 8),
                _ => ("1".into(), 8),
            },
            _ => {
                let parts: Vec<String> = args.iter().map(|x| self.ex(x, 0)).collect();
                (format!("{}({})", id(name), parts.join(", ")), 8)
            }
        }
    }

    fn macro_call(
        &mut self,
        name: &str,
        var: &str,
        lo: &Expr,
        hi: Option<&Expr>,
        args: &[Expr],
    ) -> (String, u8) {
        let Some(hi) = hi else {
            self.note(format!("`{name}` without an end can't be translated"));
            return ("default".into(), 8);
        };
        let v = id(var);
        let canon = crate::parser::macro_alias(name).unwrap_or("");
        let call = match (name, canon) {
            ("COUNT" | "DARAB", _) | (_, "COUNT") if !args.is_empty() => {
                format!("Count({v} => {})", self.ex(&args[0], 0))
            }
            ("SUM", _) if !args.is_empty() => {
                if let Some(c) = args.get(1) {
                    format!(
                        "Where({v} => {}).Sum({v} => {})",
                        self.ex(c, 0),
                        self.ex(&args[0], 0)
                    )
                } else {
                    format!("Sum({v} => {})", self.ex(&args[0], 0))
                }
            }
            ("EXISTS", _) | (_, "EXISTS") if !args.is_empty() => {
                format!("Any({v} => {})", self.ex(&args[0], 0))
            }
            ("FORALL", _) | (_, "FORALL") if !args.is_empty() => {
                format!("All({v} => {})", self.ex(&args[0], 0))
            }
            _ => {
                self.note(format!(
                    "`{name}` can't be written as an expression: draw it as a loop in the structogram"
                ));
                return ("default".into(), 8);
            }
        };
        self.uses_linq = true;
        self.note("an expression with a pattern (SUM, COUNT, …) became a LINQ call");
        let range = self.range(lo, hi);
        (format!("{range}.{call}"), 8)
    }

    /// `hi + 1` — the length of an array that ends at `hi`.
    fn plus_one(&mut self, hi: &Expr) -> String {
        match &*hi.kind {
            ExprKind::Int(k) => (k + 1).to_string(),
            ExprKind::Bin(BinOp::Sub, x, one) if matches!(&*one.kind, ExprKind::Int(1)) => {
                self.ex(x, 0)
            }
            _ => format!("{} + 1", self.ex(hi, 5)),
        }
    }

    /// Does `e` only use inputs (so that it has a value before the algorithm runs)?
    fn only_inputs(&self, e: &Expr) -> bool {
        vars_in(e)
            .iter()
            .all(|v| self.spec.decl(v).is_some_and(|d| d.role == Role::In))
    }

    fn size(&mut self, hi: Option<&Expr>) -> String {
        match hi {
            Some(h) if self.only_inputs(h) => self.plus_one(h),
            _ => self.fallback.clone(),
        }
    }

    /// `new int[n + 1]` for an array of type `t`.
    fn alloc(&mut self, t: &Type) -> String {
        let spec = self.spec;
        let (dims, elem) = dims_of(spec, t);
        let elem = self.ty_of(elem);
        let sizes: Vec<String> = dims.iter().map(|(_, hi)| self.size(*hi)).collect();
        format!("new {}[{}]", self.cs(&elem), sizes.join(", "))
    }

    // ------------------------------------------------------------------ structograms

    fn fragment(&mut self, text: &str) -> Result<Fragment, String> {
        let from = self.src.len();
        self.src.push_str(text);
        self.src.push('\n');
        parse_fragment(&self.src, from, from + text.len()).map_err(|d| d.title)
    }

    fn lower(&mut self, nodes: &[Node]) -> Vec<Ir> {
        let mut out = vec![];
        for n in nodes {
            match &n.kind {
                Kind::Noop => {}
                Kind::Statement(t) if matches!(t.text.trim(), "" | "-") || is_io(&t.text) => {}
                Kind::Statement(t) => match self.fragment(&t.text) {
                    Ok(Fragment::Assign(a, b)) => out.push(Ir::Assign(a, b)),
                    Ok(_) => out.push(self.todo(&t.text, "it is not an assignment")),
                    Err(e) => out.push(self.todo(&t.text, &e)),
                },
                Kind::If { cond, yes, no } => match self.cond(&cond.text) {
                    Ok(c) => {
                        let (y, n) = (self.lower(yes), self.lower(no));
                        out.push(Ir::If(c, y, n));
                    }
                    Err(e) => out.push(self.todo(&cond.text, &e)),
                },
                Kind::While { cond, body } => match self.fragment(&cond.text) {
                    Ok(Fragment::Range(v, lo, hi)) => {
                        let b = self.lower(body);
                        out.push(Ir::Count(v, lo, hi, b));
                    }
                    Ok(Fragment::Expr(c)) => {
                        let b = self.lower(body);
                        out.push(Ir::While(c, b));
                    }
                    Ok(_) => out.push(self.todo(&cond.text, "it is not a condition")),
                    Err(e) => out.push(self.todo(&cond.text, &e)),
                },
                Kind::Repeat { cond, body } => match self.cond(&cond.text) {
                    Ok(c) => {
                        let b = self.lower(body);
                        out.push(Ir::Repeat(c, b));
                    }
                    Err(e) => out.push(self.todo(&cond.text, &e)),
                },
                Kind::Switch { cases } => {
                    let mut cs = vec![];
                    for c in cases {
                        let cond = match &c.cond {
                            Some(t) => match self.cond(&t.text) {
                                Ok(e) => Some(e),
                                Err(e) => {
                                    cs.push((None, vec![self.todo(&t.text, &e)]));
                                    continue;
                                }
                            },
                            None => None,
                        };
                        let b = self.lower(&c.body);
                        cs.push((cond, b));
                    }
                    out.push(Ir::Switch(cs));
                }
            }
        }
        self.merge_ir(out)
    }

    /// Decisions that follow each other on one condition (or on its opposite) become one:
    /// `if (c) { A } if (!c) { B }` → `if (c) { A } else { B }`. Only when the first one leaves
    /// what the condition reads alone.
    fn merge_ir(&self, ir: Vec<Ir>) -> Vec<Ir> {
        let mut out: Vec<Ir> = vec![];
        for s in ir {
            let rel = match (out.last(), &s) {
                (Some(Ir::If(c1, y1, n1)), Ir::If(c2, ..)) => {
                    let text = |e: &Expr| &self.src[e.start..e.end];
                    let (mut assigned, mut counts) = (vec![], vec![]);
                    Self::scan(y1, &mut assigned, &mut counts);
                    Self::scan(n1, &mut assigned, &mut counts);
                    let touched = vars_in(c1)
                        .iter()
                        .any(|v| counts.contains(v) || assigned.iter().any(|(a, _)| a == v));
                    if touched {
                        None
                    } else {
                        relation(text(c1), text(c2))
                    }
                }
                _ => None,
            };
            match (rel, s) {
                (Some(rel), Ir::If(_, y2, n2)) => {
                    let Some(Ir::If(c1, y1, n1)) = out.pop() else {
                        unreachable!()
                    };
                    let cat = |a: Vec<Ir>, b: Vec<Ir>| a.into_iter().chain(b).collect::<Vec<_>>();
                    let (y, n) = match rel {
                        Rel::Same => (cat(y1, y2), cat(n1, n2)),
                        Rel::Opposite => (cat(y1, n2), cat(n1, y2)),
                    };
                    out.push(Ir::If(c1, self.merge_ir(y), self.merge_ir(n)));
                }
                (_, s) => out.push(s),
            }
        }
        out
    }

    fn cond(&mut self, text: &str) -> Result<Expr, String> {
        match self.fragment(text)? {
            Fragment::Expr(e) => Ok(e),
            _ => Err("it is not a condition".into()),
        }
    }

    fn todo(&mut self, text: &str, why: &str) -> Ir {
        self.note(format!("`{}` can't be translated: {why}", text.trim()));
        Ir::Todo(format!("{} ({why})", text.trim()))
    }

    /// Assigned variables (with the first value they get) and counting-loop variables.
    #[allow(clippy::type_complexity)]
    fn scan<'i>(ir: &'i [Ir], assigned: &mut Vec<(String, &'i Expr)>, counts: &mut Vec<String>) {
        for s in ir {
            match s {
                Ir::Assign(t, v) => {
                    let mut names = vec![];
                    target_names(t, &mut names);
                    for n in names {
                        if !assigned.iter().any(|(a, _)| *a == n) {
                            assigned.push((n, v));
                        }
                    }
                }
                Ir::If(_, a, b) => {
                    Self::scan(a, assigned, counts);
                    Self::scan(b, assigned, counts);
                }
                Ir::Count(v, _, _, b) => {
                    if !counts.contains(v) {
                        counts.push(v.clone());
                    }
                    Self::scan(b, assigned, counts);
                }
                Ir::While(_, b) | Ir::Repeat(_, b) => Self::scan(b, assigned, counts),
                Ir::Switch(cs) => cs.iter().for_each(|(_, b)| Self::scan(b, assigned, counts)),
                Ir::Todo(_) => {}
            }
        }
    }

    /// The variables of a structogram that the specification doesn't declare, in the order
    /// they should be declared: its variable list, then the ones it assigns.
    fn locals(&mut self, d: &Diagram, ir: &[Ir], skip: &[String]) -> Vec<(String, Ty)> {
        let mut assigned = vec![];
        let mut counts = vec![];
        Self::scan(ir, &mut assigned, &mut counts);
        let mut out: Vec<(String, Ty)> = vec![];
        for item in d.variables.text.split([',', ';', '\n']) {
            let (name, word) = match item.split_once(':') {
                Some((n, w)) => (n.trim(), w.trim()),
                None => (item.trim(), ""),
            };
            if name.is_empty()
                || self.vars.contains_key(name)
                || skip.iter().any(|s| s == name)
                || out.iter().any(|(n, _)| n == name)
            {
                continue;
            }
            let ty = self.word_ty(word).unwrap_or_else(|| {
                if !word.is_empty() {
                    self.note(format!(
                        "the type `{word}` of `{name}` is not known: used `int`"
                    ));
                }
                Ty::Int
            });
            out.push((name.to_string(), ty));
        }
        for (name, value) in assigned {
            if self.vars.contains_key(&name)
                || skip.contains(&name)
                || out.iter().any(|(n, _)| *n == name)
            {
                continue;
            }
            let ty = self.kind(value);
            out.push((name, ty));
        }
        // a variable only counted with needs no declaration: `for (int i = …`
        let assigned_names: HashSet<String> = {
            let mut a = vec![];
            Self::scan(ir, &mut a, &mut vec![]);
            a.into_iter().map(|(n, _)| n).collect()
        };
        out.retain(|(n, _)| !(counts.contains(n) && !assigned_names.contains(n)));
        out
    }

    /// How C# prints a value with `Console.Write`.
    fn shown(v: &Value) -> String {
        match v {
            Value::Bool(true) => "True".into(),
            Value::Bool(false) => "False".into(),
            other => Self::plain(other),
        }
    }

    fn shown_elem(&self, v: &Value, ty: &Ty) -> String {
        match (v, ty) {
            (Value::Rec(fs), Ty::Rec(n)) => self
                .fields_of(n)
                .iter()
                .map(|(f, _)| {
                    fs.iter()
                        .find(|(x, _)| x == f)
                        .map_or(String::new(), |(_, x)| Self::shown(x))
                })
                .collect::<Vec<_>>()
                .join(" "),
            _ => Self::shown(v),
        }
    }

    /// The console text for one output (the counterpart of [`Gen::print_decl`]).
    fn output_lines(&self, v: &Value, ty: &Ty, out: &mut String) {
        match (v, ty) {
            (Value::Arr { items, .. }, Ty::Arr { dims: 1, elem, .. }) => {
                for it in items.iter() {
                    out.push_str(&self.shown_elem(it, elem));
                    out.push(' ');
                }
                out.push('\n');
            }
            (Value::Arr { items, .. }, Ty::Arr { dims: 2, elem, .. }) => {
                for row in items.iter() {
                    if let Value::Arr { items, .. } = row {
                        for it in items.iter() {
                            out.push_str(&self.shown_elem(it, elem));
                            out.push(' ');
                        }
                    }
                    out.push('\n');
                }
            }
            _ => {
                out.push_str(&self.shown_elem(v, ty));
                out.push('\n');
            }
        }
    }

    // ------------------------------------------------------------------ output

    fn line(&mut self, s: &str) {
        if s.is_empty() {
            self.out.push('\n');
        } else {
            self.out.push_str(&"    ".repeat(self.depth));
            self.out.push_str(s);
            self.out.push('\n');
        }
    }

    fn open(&mut self) {
        self.line("{");
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth -= 1;
        self.line("}");
    }

    fn block(&mut self, ir: &[Ir]) {
        self.open();
        self.emit(ir);
        self.close();
    }

    fn emit(&mut self, ir: &[Ir]) {
        for s in ir {
            match s {
                Ir::Assign(t, v) => {
                    let want = self.kind(t);
                    let (t, v) = (self.ex(t, 0), self.ex_want(v, 0, &want));
                    self.line(&format!("{t} = {v};"));
                }
                Ir::If(c, yes, no) => {
                    let c = self.ex(c, 0);
                    if yes.is_empty() && !no.is_empty() {
                        self.line(&format!("if (!({c}))"));
                        self.block(no);
                    } else {
                        self.line(&format!("if ({c})"));
                        self.block(yes);
                        if !no.is_empty() {
                            self.line("else");
                            self.block(no);
                        }
                    }
                }
                Ir::Count(v, lo, hi, body) => {
                    let new = !self.vars.contains_key(v);
                    let (l, h) = (self.ex(lo, 0), self.ex(hi, 5));
                    let name = id(v);
                    let int = if new { "int " } else { "" };
                    self.line(&format!("for ({int}{name} = {l}; {name} <= {h}; {name}++)"));
                    if new {
                        self.vars.insert(v.clone(), Ty::Int);
                    }
                    self.block(body);
                    if new {
                        self.vars.remove(v);
                    }
                }
                Ir::While(c, body) => {
                    let c = self.ex(c, 0);
                    self.line(&format!("while ({c})"));
                    self.block(body);
                }
                Ir::Repeat(c, body) => {
                    let c = self.ex(c, 0);
                    self.line("do");
                    self.open();
                    self.emit(body);
                    self.depth -= 1;
                    self.line(&format!("}} while ({c});"));
                }
                Ir::Switch(cases) => {
                    for (k, (c, body)) in cases.iter().enumerate() {
                        match c {
                            Some(c) => {
                                let c = self.ex(c, 0);
                                let kw = if k == 0 { "if" } else { "else if" };
                                self.line(&format!("{kw} ({c})"));
                            }
                            None if k == 0 => {
                                self.line("{");
                                self.depth += 1;
                                self.emit(body);
                                self.close();
                                continue;
                            }
                            None => self.line("else"),
                        }
                        self.block(body);
                    }
                }
                Ir::Todo(t) => self.line(&format!("// TODO: {t}")),
            }
        }
    }
}

// ---------------------------------------------------------------------------- the program

const SPLIT: &str = "Console.ReadLine().Split(' ', StringSplitOptions.RemoveEmptyEntries)";

impl<'a> Gen<'a> {
    fn run(
        mut self,
        program: &Program,
        inputs: Inputs,
        env: Option<Env>,
    ) -> Result<Output, String> {
        self.labels = inputs == Inputs::Test;
        let spec = self.spec;
        let headers: Vec<_> = program
            .diagrams
            .iter()
            .map(|d| header(&d.name.text))
            .collect();
        let main = headers
            .iter()
            .position(Option::is_none)
            .or((!headers.is_empty()).then_some(0))
            .ok_or("There is no structogram to translate")?;
        let funcs: Vec<(usize, String, Vec<String>)> = headers
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != main)
            .filter_map(|(k, h)| h.clone().map(|(n, p)| (k, n, p)))
            .collect();
        for (k, d) in program.diagrams.iter().enumerate() {
            if k != main && headers[k].is_none() {
                self.note(format!(
                    "the structogram `{}` is neither the main one nor a function: left out",
                    d.name.text.trim()
                ));
            }
        }
        self.statics = !funcs.is_empty() || !spec.funcs.is_empty();

        // every structogram, parsed once
        let mut irs: HashMap<usize, Vec<Ir>> = HashMap::new();
        for k in std::iter::once(main).chain(funcs.iter().map(|f| f.0)) {
            let ir = self.lower(&program.diagrams[k].body);
            irs.insert(k, ir);
        }
        for d in program.diagrams.iter() {
            for item in d.variables.text.split([',', ';', '\n']) {
                self.taken
                    .insert(item.split(':').next().unwrap_or("").trim().to_string());
            }
        }
        for ir in irs.values() {
            let (mut a, mut c) = (vec![], vec![]);
            Self::scan(ir, &mut a, &mut c);
            self.taken.extend(a.into_iter().map(|(n, _)| n));
            self.taken.extend(c);
        }
        self.pick_fallback();

        // ---- Main
        self.depth = 2;
        let (kw_in, kw_out) = if self.english {
            ("In", "Out")
        } else {
            ("Be", "Ki")
        };
        let ins: Vec<_> = spec.decls.iter().filter(|d| d.role == Role::In).collect();
        let outs: Vec<_> = spec.decls.iter().filter(|d| d.role == Role::Out).collect();
        let others: Vec<_> = spec.decls.iter().filter(|d| d.role != Role::In).collect();
        let names = |v: &[&crate::parser::Decl]| {
            v.iter()
                .map(|d| d.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !ins.is_empty() {
            self.line(&format!("// {kw_in}: {}", names(&ins)));
            match &env {
                Some(env) => ins.iter().for_each(|d| self.test_decl(d, env)),
                None => {
                    let needs = ins
                        .iter()
                        .any(|d| self.vars.get(&d.name).is_some_and(|t| *t != Ty::Str));
                    let sor = self.fresh("sor");
                    if needs {
                        self.line(&format!("string[] {sor};"));
                    }
                    ins.iter().for_each(|d| self.read_decl(d, &sor));
                }
            }
            self.line("");
        }
        let mut vars: Vec<Var> = vec![];
        for d in &others {
            let ty = self.vars[&d.name].clone();
            if !self.statics {
                vars.push(Var {
                    name: d.name.clone(),
                    ty,
                    decl: Some(d),
                });
            } else if matches!(ty, Ty::Arr { .. }) {
                // a field: its array is made here
                let alloc = self.alloc(&d.ty);
                self.line(&format!("{} = {alloc};", id(&d.name)));
            }
        }
        let main_d = &program.diagrams[main];
        let main_ir = irs.remove(&main).unwrap_or_default();
        for (name, ty) in self.locals(main_d, &main_ir, &[]) {
            vars.push(Var {
                name,
                ty,
                decl: None,
            });
        }
        self.declare_and_emit(vars, &main_ir, true);
        if !outs.is_empty() {
            self.line("");
            self.line(&format!("// {kw_out}: {}", names(&outs)));
            for d in &outs {
                self.print_decl(d);
            }
            if let Some(env) = &env {
                for d in &outs {
                    if let Some(v) = env.get(&d.name) {
                        let w = if self.english {
                            "Expected (test case)"
                        } else {
                            "Várt (teszteset)"
                        };
                        self.line(&format!("// {w}: {} = {v}", d.name));
                    }
                }
            }
        }
        let main_text = std::mem::take(&mut self.out);

        // ---- functions
        self.depth = 1;
        for (k, name, params) in &funcs {
            self.function(
                &program.diagrams[*k],
                name,
                params,
                &irs.remove(k).unwrap_or_default(),
            );
        }
        let mut stubs: Vec<_> = spec
            .funcs
            .keys()
            .filter(|n| !funcs.iter().any(|f| &f.1 == *n))
            .cloned()
            .collect();
        stubs.sort();
        for name in stubs {
            self.stub(&name);
        }
        if self.uses_fact {
            self.line("");
            self.line("static long Factorial(int n)");
            self.open();
            self.line("long r = 1;");
            self.line("for (int i = 2; i <= n; i++)");
            self.open();
            self.line("r = r * i;");
            self.close();
            self.line("return r;");
            self.close();
        }
        let methods = std::mem::take(&mut self.out);

        // ---- assemble
        let mut code = String::from("using System;\n");
        if self.uses_culture {
            code.push_str("using System.Globalization;\n");
        }
        if self.uses_linq {
            code.push_str("using System.Linq;\n");
        }
        code.push_str("\nclass Program\n{\n");
        for (n, fs) in &self.structs {
            let body: Vec<String> = fs
                .iter()
                .map(|(f, t)| format!("public {} {};", self.cs(t), id(f)))
                .collect();
            code.push_str(&format!("    struct {n} {{ {} }}\n", body.join(" ")));
        }
        if !self.structs.is_empty() {
            code.push('\n');
        }
        let fields = self.field_lines();
        if !fields.is_empty() {
            code.push_str(&fields);
            code.push('\n');
        }
        code.push_str("    static void Main()\n    {\n");
        code.push_str(&main_text);
        code.push_str("    }\n");
        code.push_str(&methods);
        code.push_str("}\n");
        Ok(Output {
            code,
            notes: self.notes,
        })
    }

    /// Size of arrays whose length depends on something computed: that of the first input array.
    fn pick_fallback(&mut self) {
        let spec = self.spec;
        for d in spec.decls.iter().filter(|d| d.role == Role::In) {
            let (dims, _) = dims_of(spec, &d.ty);
            if let Some((_, Some(hi))) = dims.first()
                && self.only_inputs(hi)
            {
                self.fallback = self.plus_one(hi);
                return;
            }
        }
    }

    /// What a variable starts with when it is declared before its first step.
    fn init_of(&mut self, v: &Var) -> String {
        match (&v.ty, v.decl) {
            (Ty::Arr { .. }, Some(d)) => self.alloc(&d.ty),
            (Ty::Arr { dims, elem, .. }, None) => {
                self.note(format!(
                    "the length of the array `{}` is not known: {} used",
                    v.name, self.fallback
                ));
                let size = vec![self.fallback.clone(); *dims].join(", ");
                format!("new {}[{size}]", self.cs(elem))
            }
            (Ty::Rec(n), _) => format!("new {n}()"),
            (t, _) => self.default_lit(t),
        }
    }

    /// Declares the variables of a routine and writes its steps. A variable whose first step is
    /// to get its value is declared with it (`int i = 1;`), as long as that value reads nothing
    /// that is not declared yet; the others are declared first, with a starting value.
    fn declare_and_emit(&mut self, vars: Vec<Var>, ir: &[Ir], blank: bool) {
        // the plain assignments the routine starts with
        let lead: Vec<(&Expr, &Expr, &str)> = ir
            .iter()
            .map_while(|s| match s {
                Ir::Assign(t, v) => match &*t.kind {
                    ExprKind::Var(n) => Some((t, v, n.as_str())),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        // variables of this routine whose first step is in `lead`, until their step comes
        let mut waiting: Vec<&str> = vec![];
        for (_, _, n) in &lead {
            if vars.iter().any(|v| v.name == *n) && !waiting.contains(n) {
                waiting.push(n);
            }
        }
        let mut merged = vec![false; lead.len()];
        for (k, (_, value, n)) in lead.iter().enumerate() {
            if !waiting.contains(n) {
                continue;
            }
            // what the value reads must be declared before this step: a variable that is
            // still waiting for its own step is declared first instead (`a = b + 1; b = 2;`)
            let reads = vars_in(value);
            waiting.retain(|w| !reads.iter().any(|r| r == w || r == n) || w == n);
            // `i = i + 1` reads itself: it needs its starting value
            merged[k] = !reads.contains(&n.to_string());
            waiting.retain(|w| w != n);
        }
        let merged_names: Vec<&str> = lead
            .iter()
            .zip(&merged)
            .filter(|(_, m)| **m)
            .map(|((_, _, n), _)| *n)
            .collect();
        let mut any = false;
        for v in vars
            .iter()
            .filter(|v| !merged_names.contains(&v.name.as_str()))
        {
            let init = self.init_of(v);
            self.vars.insert(v.name.clone(), v.ty.clone());
            self.line(&format!("{} {} = {init};", self.cs(&v.ty), id(&v.name)));
            any = true;
        }
        if any && blank {
            self.line("");
        }
        for (k, (t, value, n)) in lead.iter().enumerate() {
            if merged[k] {
                let ty = vars.iter().find(|v| v.name == *n).unwrap().ty.clone();
                let value = self.ex_want(value, 0, &ty);
                self.vars.insert(n.to_string(), ty.clone());
                self.line(&format!("{} {} = {value};", self.cs(&ty), id(n)));
            } else {
                let _ = t;
                self.emit(&ir[k..=k]);
            }
        }
        self.emit(&ir[lead.len()..]);
    }

    /// The fields of `Program`, for a specification with functions.
    fn field_lines(&self) -> String {
        if !self.statics {
            return String::new();
        }
        let mut scalars: Vec<(String, Vec<String>)> = vec![];
        let mut out = String::new();
        for d in &self.spec.decls {
            let ty = &self.vars[&d.name];
            let cs = self.cs(ty);
            match ty {
                Ty::Str => out.push_str(&format!("    static string {} = \"\";\n", id(&d.name))),
                _ => match scalars.iter_mut().find(|(t, _)| *t == cs) {
                    Some((_, v)) => v.push(id(&d.name)),
                    None => scalars.push((cs, vec![id(&d.name)])),
                },
            }
        }
        let mut head = String::new();
        for (t, names) in scalars {
            head.push_str(&format!("    static {t} {};\n", names.join(", ")));
        }
        head + &out
    }

    // ------------------------------------------------------------------ functions

    fn param_types(&mut self, name: &str, n: usize) -> Vec<Ty> {
        let doms = self
            .spec
            .funcs
            .get(name)
            .map(|f| f.dom.clone())
            .unwrap_or_default();
        (0..n)
            .map(|i| match doms.get(i) {
                Some(t) if doms.len() == n => self.ty_of(t),
                _ => Ty::Int,
            })
            .collect()
    }

    fn return_type(&mut self, name: &str) -> Option<String> {
        let cod = self.spec.funcs.get(name)?.cod.clone();
        let tys: Vec<String> = cod
            .iter()
            .map(|t| {
                let t = self.ty_of(t);
                self.cs(&t)
            })
            .collect();
        match tys.len() {
            0 => None,
            1 => Some(tys[0].clone()),
            _ => Some(format!("({})", tys.join(", "))),
        }
    }

    fn function(&mut self, d: &Diagram, name: &str, params: &[String], ir: &[Ir]) {
        let saved = std::mem::replace(&mut self.vars, self.base_vars.clone());
        let ptys = self.param_types(name, params.len());
        for (p, t) in params.iter().zip(&ptys) {
            self.vars.insert(p.clone(), t.clone());
        }
        let ret_text = d.ret.text.trim();
        let ret = (d.show_return && !ret_text.is_empty())
            .then(|| self.cond(ret_text))
            .transpose();
        let ret = match ret {
            Ok(r) => r,
            Err(e) => {
                self.note(format!(
                    "the result `{ret_text}` of `{name}` can't be translated: {e}"
                ));
                None
            }
        };
        let ret_ty = self
            .return_type(name)
            .or_else(|| {
                ret.as_ref().map(|e| {
                    let t = self.kind(e);
                    self.cs(&t)
                })
            })
            .unwrap_or_else(|| "void".into());
        let plist: Vec<String> = params
            .iter()
            .zip(&ptys)
            .map(|(p, t)| format!("{} {}", self.cs(t), id(p)))
            .collect();
        self.line("");
        self.line(&format!(
            "static {ret_ty} {}({})",
            id(name),
            plist.join(", ")
        ));
        self.open();
        let vars: Vec<Var> = self
            .locals(d, ir, params)
            .into_iter()
            .map(|(name, ty)| Var {
                name,
                ty,
                decl: None,
            })
            .collect();
        self.declare_and_emit(vars, ir, false);
        if let Some(r) = &ret {
            let r = self.ex(r, 0);
            self.line(&format!("return {r};"));
        }
        self.close();
        self.vars = saved;
    }

    /// A function of the specification that has no structogram: its definition as one `return`.
    fn stub(&mut self, name: &str) {
        let Some(f) = self.spec.funcs.get(name).cloned() else {
            return;
        };
        let saved = std::mem::replace(&mut self.vars, self.base_vars.clone());
        let ptys = self.param_types(name, f.params.len());
        for (p, t) in f.params.iter().zip(&ptys) {
            self.vars.insert(p.clone(), t.clone());
        }
        let ret = self.return_type(name).unwrap_or_else(|| {
            let t = self.kind(&f.body);
            self.cs(&t)
        });
        let plist: Vec<String> = f
            .params
            .iter()
            .zip(&ptys)
            .map(|(p, t)| format!("{} {}", self.cs(t), id(p)))
            .collect();
        let body = self.ex(&f.body, 0);
        self.line("");
        self.line(&format!("static {ret} {}({})", id(name), plist.join(", ")));
        self.open();
        self.line(&format!("return {body};"));
        self.close();
        self.vars = saved;
        self.note(format!(
            "`{name}` has no structogram: its definition from the specification is used"
        ));
    }

    // ------------------------------------------------------------------ input

    fn prefix(&self, ty: &Ty) -> String {
        if self.statics {
            String::new()
        } else {
            format!("{} ", self.cs(ty))
        }
    }

    fn parse(&mut self, ty: &Ty, x: &str) -> String {
        match ty {
            Ty::Int => format!("int.Parse({x})"),
            Ty::Real => {
                self.uses_culture = true;
                format!("double.Parse({x}, CultureInfo.InvariantCulture)")
            }
            Ty::Bool => format!("({x} == \"igaz\" || {x} == \"true\" || {x} == \"True\")"),
            Ty::Char => format!("{x}[0]"),
            _ => x.to_string(),
        }
    }

    /// `v - lo`: the position in the line of the value of index `v`.
    fn offset(&mut self, var: &str, lo: &Expr) -> String {
        match &*lo.kind {
            ExprKind::Int(0) => var.to_string(),
            ExprKind::Int(k) => format!("{var} - {k}"),
            _ => format!("{var} - {}", self.ex(lo, 6)),
        }
    }

    fn read_fields(&mut self, base: &str, rec: &str, sor: &str) {
        for (k, (f, ft)) in self.fields_of(rec).into_iter().enumerate() {
            if ft.is_scalar() {
                let p = self.parse(&ft, &format!("{sor}[{k}]"));
                self.line(&format!("{base}.{} = {p};", id(&f)));
            } else {
                self.note(format!(
                    "the field `{f}` of `{rec}` is not a simple value: it is not read"
                ));
            }
        }
    }

    fn read_decl(&mut self, d: &crate::parser::Decl, sor: &str) {
        let spec = self.spec;
        let ty = self.vars[&d.name].clone();
        let name = id(&d.name);
        let pre = self.prefix(&ty);
        match &ty {
            Ty::Str => self.line(&format!("{pre}{name} = Console.ReadLine();")),
            t if t.is_scalar() => {
                self.line(&format!("{sor} = {SPLIT};"));
                let p = self.parse(t, &format!("{sor}[0]"));
                self.line(&format!("{pre}{name} = {p};"));
            }
            Ty::Rec(n) => {
                self.line(&format!("{pre}{name} = new {n}();"));
                self.line(&format!("{sor} = {SPLIT};"));
                self.read_fields(&name, n, sor);
            }
            Ty::Arr { dims, elem, .. } => {
                let alloc = self.alloc(&d.ty);
                self.line(&format!("{pre}{name} = {alloc};"));
                let (dd, _) = dims_of(spec, &d.ty);
                if *dims > 2 {
                    self.note(format!(
                        "`{}` has more than two dimensions: it is not read",
                        d.name
                    ));
                    return;
                }
                let mut vars = vec![];
                let mut bounds = vec![];
                for (lo, hi) in &dd {
                    let v = self.fresh(if vars.is_empty() { "j" } else { "q" });
                    let l = self.ex(lo, 0);
                    let h = match hi {
                        Some(h) => self.ex(h, 5),
                        None => format!("{name}.Length - 1"),
                    };
                    vars.push(v.clone());
                    bounds.push((v, l, h, lo));
                }
                let last = bounds.len() - 1;
                for (k, (v, l, h, _)) in bounds.iter().enumerate() {
                    // the line with the values: once, or once per row of a two-dimensional array
                    if k == last && !matches!(**elem, Ty::Rec(_)) {
                        self.line(&format!("{sor} = {SPLIT};"));
                    }
                    self.line(&format!("for (int {v} = {l}; {v} <= {h}; {v}++)"));
                    self.open();
                }
                let idx: Vec<String> = bounds.iter().map(|b| b.0.clone()).collect();
                let target = format!("{name}[{}]", idx.join(", "));
                match &**elem {
                    Ty::Rec(r) => {
                        self.line(&format!("{sor} = {SPLIT};"));
                        self.read_fields(&target, r, sor);
                    }
                    e => {
                        let pos = {
                            let (v, _, _, lo) = &bounds[last];
                            self.offset(v, lo)
                        };
                        let p = self.parse(e, &format!("{sor}[{pos}]"));
                        self.line(&format!("{target} = {p};"));
                    }
                }
                for _ in &bounds {
                    self.close();
                }
                for v in vars {
                    self.taken.remove(&v);
                }
            }
            _ => {}
        }
    }

    // A one-dimensional array reads its single line before the loop (above); a
    // two-dimensional one reads one line per row inside the outer loop.

    fn test_decl(&mut self, d: &crate::parser::Decl, env: &Env) {
        let ty = self.vars[&d.name].clone();
        let pre = self.prefix(&ty);
        let name = id(&d.name);
        match env.get(&d.name) {
            Some(v) => {
                let l = self.lit(v, &ty);
                self.line(&format!("{pre}{name} = {l};"));
            }
            None => {
                self.note(format!("the test case has no value for `{}`", d.name));
                let init = self.default_lit(&ty);
                self.line(&format!("{pre}{name} = {init};"));
            }
        }
    }

    fn lit(&mut self, v: &Value, ty: &Ty) -> String {
        match (v, ty) {
            (Value::Int(i), Ty::Real) => real(*i as f64),
            (Value::Int(i), _) => i.to_string(),
            (Value::Real(f), _) => real(*f),
            (Value::Bool(b), _) => b.to_string(),
            (Value::Str(s), Ty::Char) => char_lit(s),
            (Value::Str(s), _) => quote(s),
            (Value::Arr { from, items }, Ty::Arr { dims: 1, elem, .. }) => {
                let pad = self.default_lit(elem);
                let mut parts = vec![pad; (*from).max(0) as usize];
                for it in items.iter() {
                    parts.push(self.lit(it, elem));
                }
                format!("new {}[] {{ {} }}", self.cs(elem), parts.join(", "))
            }
            (Value::Arr { from, items }, Ty::Arr { dims: 2, elem, .. }) => {
                let pad = self.default_lit(elem);
                let rows: Vec<(usize, Vec<String>)> = items
                    .iter()
                    .map(|r| match r {
                        Value::Arr { from, items } => (
                            (*from).max(0) as usize,
                            items.iter().map(|x| self.lit(x, elem)).collect(),
                        ),
                        _ => (0, vec![]),
                    })
                    .collect();
                let width = rows.iter().map(|(f, r)| f + r.len()).max().unwrap_or(0);
                let fill = |lead: usize, vals: &[String]| {
                    let mut v = vec![pad.clone(); lead];
                    v.extend(vals.iter().cloned());
                    v.resize(width, pad.clone());
                    format!("{{ {} }}", v.join(", "))
                };
                let mut all: Vec<String> = (0..(*from).max(0)).map(|_| fill(0, &[])).collect();
                all.extend(rows.iter().map(|(f, r)| fill(*f, r)));
                format!("new {}[,] {{ {} }}", self.cs(elem), all.join(", "))
            }
            (Value::Rec(fs), Ty::Rec(n)) => {
                let tys = self.fields_of(n);
                let parts: Vec<String> = fs
                    .iter()
                    .map(|(f, val)| {
                        let t = tys
                            .iter()
                            .find(|(x, _)| x == f)
                            .map_or(Ty::Int, |x| x.1.clone());
                        format!("{} = {}", id(f), self.lit(val, &t))
                    })
                    .collect();
                format!("new {n} {{ {} }}", parts.join(", "))
            }
            _ => {
                self.note("a value could not be written into the program");
                "default".into()
            }
        }
    }

    fn plain(v: &Value) -> String {
        match v {
            Value::Int(i) => i.to_string(),
            Value::Real(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => s.clone(),
            other => other.to_string(),
        }
    }

    fn rec_line(&self, v: &Value, ty: &Ty) -> String {
        let (Value::Rec(fs), Ty::Rec(n)) = (v, ty) else {
            return Self::plain(v);
        };
        self.fields_of(n)
            .iter()
            .map(|(f, _)| {
                fs.iter()
                    .find(|(x, _)| x == f)
                    .map_or(String::new(), |(_, x)| Self::plain(x))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The console text for one input (the counterpart of [`Gen::read_decl`]).
    fn input_lines(&self, v: &Value, ty: &Ty, out: &mut String) {
        match (v, ty) {
            (Value::Arr { items, .. }, Ty::Arr { dims: 1, elem, .. }) => {
                if matches!(**elem, Ty::Rec(_)) {
                    for it in items.iter() {
                        out.push_str(&self.rec_line(it, elem));
                        out.push('\n');
                    }
                } else {
                    let line: Vec<String> = items.iter().map(Self::plain).collect();
                    out.push_str(&line.join(" "));
                    out.push('\n');
                }
            }
            (Value::Arr { items, .. }, Ty::Arr { dims: 2, elem, .. }) => {
                for row in items.iter() {
                    if let Value::Arr { items, .. } = row {
                        let line: Vec<String> =
                            items.iter().map(|x| self.rec_line(x, elem)).collect();
                        out.push_str(&line.join(" "));
                    }
                    out.push('\n');
                }
            }
            (Value::Rec(_), Ty::Rec(_)) => {
                out.push_str(&self.rec_line(v, ty));
                out.push('\n');
            }
            _ => {
                out.push_str(&Self::plain(v));
                out.push('\n');
            }
        }
    }

    // ------------------------------------------------------------------ output

    fn rec_expr(&self, base: &str, rec: &str) -> String {
        let parts: Vec<String> = self
            .fields_of(rec)
            .iter()
            .map(|(f, _)| format!("{base}.{}", id(f)))
            .collect();
        if parts.len() == 1 {
            parts[0].clone()
        } else {
            parts.join(" + \" \" + ")
        }
    }

    fn print_decl(&mut self, d: &crate::parser::Decl) {
        let spec = self.spec;
        let ty = self.vars[&d.name].clone();
        let name = id(&d.name);
        let label = self.labels;
        match &ty {
            Ty::Arr { dims, elem, .. } if *dims <= 2 => {
                if label {
                    self.line(&format!(
                        "Console.Write({});",
                        quote(&format!("{} = ", d.name))
                    ));
                }
                let (dd, _) = dims_of(spec, &d.ty);
                let mut bounds = vec![];
                for (lo, hi) in &dd {
                    let v = self.fresh(if bounds.is_empty() { "q" } else { "r" });
                    let l = self.ex(lo, 0);
                    let h = match hi {
                        Some(h) => self.ex(h, 5),
                        None => format!("{name}.Length - 1"),
                    };
                    bounds.push((v, l, h));
                }
                for (v, l, h) in &bounds {
                    self.line(&format!("for (int {v} = {l}; {v} <= {h}; {v}++)"));
                    self.open();
                }
                let idx: Vec<String> = bounds.iter().map(|b| b.0.clone()).collect();
                let item = format!("{name}[{}]", idx.join(", "));
                let shown = match &**elem {
                    Ty::Rec(r) => self.rec_expr(&item, r),
                    _ => item,
                };
                self.line(&format!("Console.Write({shown} + \" \");"));
                for k in (0..bounds.len()).rev() {
                    self.close();
                    if k > 0 {
                        self.line("Console.WriteLine();");
                    }
                }
                if bounds.len() == 1 {
                    self.line("Console.WriteLine();");
                }
                for (v, _, _) in bounds {
                    self.taken.remove(&v);
                }
            }
            Ty::Arr { .. } => self.note(format!(
                "`{}` has more than two dimensions: it is not printed",
                d.name
            )),
            Ty::Rec(r) => {
                let e = self.rec_expr(&name, r);
                if label {
                    self.line(&format!(
                        "Console.WriteLine({} + {e});",
                        quote(&format!("{} = ", d.name))
                    ));
                } else {
                    self.line(&format!("Console.WriteLine({e});"));
                }
            }
            _ => {
                if label {
                    self.line(&format!(
                        "Console.WriteLine({} + {name});",
                        quote(&format!("{} = ", d.name))
                    ));
                } else {
                    self.line(&format!("Console.WriteLine({name});"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::stuki::from_spec::from_spec;

    const COUNT: &str = "Be:\nn∈N,\nszamok∈N[1..n]\nKi:\ndb∈N\nEf:\n-\nUf:\ndb=DARAB(i=1..n, szamok[i] mod 2 = 1)\n";
    const SEARCH: &str = "Be:\nn∈N,\nk∈N,\nar∈N[1..n]\nSa:\nvan∈L,\nind∈N\nKi:\nsorszam∈Z\nEf:\n-\nUf:\n(van,ind)=KERES(i=1..n, ar[i]<k) és (van -> sorszam=ind) és (nem van -> sorszam=-1)\n";
    const MAX: &str =
        "In:\nn∈N,\nx∈R[1..n]\nOut:\nind∈N,\nm∈R\nPre:\nn>0\nPost:\n(ind,m)=MAX(i=1..n, x[i])\n";
    const SELECT: &str = "Be:\nn∈N,\nt∈Z[1..n]\nKi:\ndb∈N,\ny∈Z[1..db]\nEf:\n-\nUf:\n(db,y)=KIVÁLOGAT(i=1..n, t[i]>0, t[i])\n";
    const WIND: &str = "In: n∈Z, k∈N, szel∈N[1..n]\nAux: found∈L\nOut: nap∈Z\nFn: nicewind: N->L, nicewind(x) = FORALL(i=1..k, 10≤szel[i+x-1]≤100)\nPre: 1≤n≤1000 and 1≤k≤10 and ∀i∈[1..n]:(0≤szel[i]≤200)\nPost: (found, nap) = SEARCH(i=1..n-(k-1), nicewind(i)) and not(found) -> nap = -1\n";
    const FUNC: &str = "Be:\nn∈N,\nm∈N,\nt∈Z[1..n,1..m]\nKi:\nbest∈Z\nFv:\nsormax:N->Z,\nsormax(i)=MAX(j=1..m, t[i,j]).2\nEf:\nn>0 és m>0\nUf:\nbest=MAX(i=1..n, sormax(i)).2\n";

    /// (name, spec, test case)
    fn cases() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            ("count", COUNT, "n: 5\nszamok: [3, 72, 81, 4, 5]\ndb: 3\n"),
            (
                "search",
                SEARCH,
                "n: 4\nk: 10\nar: [30, 5, 20, 8]\nvan: true\nind: 2\nsorszam: 2\n",
            ),
            (
                "search_none",
                SEARCH,
                "n: 3\nk: 1\nar: [30, 5, 20]\nvan: false\nind: 0\nsorszam: -1\n",
            ),
            ("max", MAX, "n: 3\nx: [1.5, 9.25, 3]\nind: 2\nm: 9.25\n"),
            (
                "select",
                SELECT,
                "n: 5\nt: [3, -1, 0, 7, 2]\ndb: 3\ny: [3, 7, 2]\n",
            ),
            (
                "wind",
                WIND,
                "n: 5\nk: 2\nszel: [5, 20, 30, 15, 200]\nfound: true\nnap: 2\n",
            ),
            (
                "wind_none",
                WIND,
                "n: 1\nk: 1\nszel: [1]\nfound: false\nnap: -1\n",
            ),
            (
                "func",
                FUNC,
                "n: 2\nm: 3\nt: [[1, 5, 2], [7, 0, 3]]\nbest: 7\n",
            ),
        ]
    }

    fn build(spec: &str) -> (Spec, Program) {
        let spec = parse(spec).unwrap_or_else(|e| panic!("{}", e[0].title));
        let program = from_spec(&spec, "T").program;
        (spec, program)
    }

    /// Writes every program to `$CSGEN_DIR` (when set) so that `dotnet` can run them.
    #[test]
    fn programs_for_dotnet() {
        let Ok(dir) = std::env::var("CSGEN_DIR") else {
            return;
        };
        std::fs::create_dir_all(&dir).unwrap();
        for (name, src, test) in cases() {
            let (spec, program) = build(src);
            for (mode, tag) in [(Inputs::Test, "test"), (Inputs::Console, "console")] {
                let out =
                    generate(&spec, &program, mode, test).unwrap_or_else(|e| panic!("{name}: {e}"));
                std::fs::write(format!("{dir}/{name}_{tag}.cs"), &out.code).unwrap();
                std::fs::write(format!("{dir}/{name}_{tag}.notes"), out.notes.join("\n")).unwrap();
            }
            std::fs::write(
                format!("{dir}/{name}_console.in"),
                console_input(&spec, test).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn count_in_the_shape_of_skill_md() {
        let (spec, program) = build(COUNT);
        let out = generate(
            &spec,
            &program,
            Inputs::Test,
            "n: 5\nszamok: [3, 72, 81, 4, 5]\ndb: 3\n",
        )
        .unwrap();
        assert_eq!(
            out.code,
            "using System;

class Program
{
    static void Main()
    {
        // Be: n, szamok
        int n = 5;
        int[] szamok = new int[] { 0, 3, 72, 81, 4, 5 };

        int db = 0;
        for (int i = 1; i <= n; i++)
        {
            if (szamok[i] % 2 == 1)
            {
                db = db + 1;
            }
        }

        // Ki: db
        Console.WriteLine(\"db = \" + db);
        // Várt (teszteset): db = 3
    }
}
"
        );
    }

    /// A hand-drawn structogram with decisions that follow each other: one `if`/`else`.
    #[test]
    fn adjacent_decisions_become_one_if_else() {
        use crate::stuki::model::{Diagram, Kind, Node, fresh_id};
        use crate::stuki::rich::Rich;
        let stmt = |t: &str| Node {
            id: fresh_id(),
            bg: None,
            kind: Kind::Statement(Rich::plain(t)),
        };
        let decision = |c: &str, y: Vec<Node>| Node {
            id: fresh_id(),
            bg: None,
            kind: Kind::If {
                cond: Rich::plain(c),
                yes: y,
                no: vec![Node::noop()],
            },
        };
        let spec = parse("In: n∈Z\nOut: r∈Z, s∈Z\nPre: -\nPost: r = n and s = n\n").unwrap();
        let build = |body: Vec<Node>| {
            let mut d = Diagram::new();
            d.body = body;
            Program {
                diagrams: vec![d],
                ..Program::default()
            }
        };
        let code = |p: &Program| {
            generate(&spec, p, Inputs::Test, "n: 1\nr: 1\ns: 1\n")
                .unwrap()
                .code
        };
        // opposite conditions
        let p = build(vec![
            decision("n>0", vec![stmt("r:=1")]),
            decision("n≤0", vec![stmt("r:=2")]),
        ]);
        let text = code(&p);
        assert_eq!(text.matches("if (").count(), 1, "{text}");
        assert!(
            text.contains("if (n > 0)") && text.contains("else"),
            "{text}"
        );
        // the first decision changes what the condition reads: they stay two
        let p = build(vec![
            decision("r>0", vec![stmt("r:=0")]),
            decision("r≤0", vec![stmt("s:=1")]),
        ]);
        assert_eq!(code(&p).matches("if (").count(), 2);
    }

    /// The link of a program, as carbon.now.sh gave it for this one.
    #[test]
    fn carbon_link_is_what_carbon_makes() {
        let code = "using System;\n\nclass Program\n{\n    static int n, pontElsoUresig;\n    static int[] pontok = new int[0];\n\n    static void Main()\n    {\n        // Be: n, pontok\n        n = 1;\n        pontok = new int[] { 0, 0 };\n\n\n        pontElsoUresig = 0;\n        for (int i = 1; i <= elsoUres(pontok) - 1; i++)\n        {\n            pontElsoUresig = pontElsoUresig + pontok[i];\n        }\n\n        // Ki: pontElsoUresig\n        Console.WriteLine(\"pontElsoUresig = \" + pontElsoUresig);\n        // Várt (teszteset): pontElsoUresig = 0\n    }\n\n    static int elsoUres(int[] pontok)\n    {\n        bool van = false;\n        int ind = 0;\n        int j = 0;\n        j = 1;\n        while (j <= n && !(pontok[j] == 0))\n        {\n            j = j + 1;\n        }\n        van = j <= n;\n        if (van)\n        {\n            ind = j;\n        }\n        return van ? ind : n;\n    }\n}\n";
        let expected = "https://carbon.now.sh/?bg=rgba%28171%2C+184%2C+195%2C+1%29&t=seti&wt=none&l=text%2Fx-csharp&width=680&ds=true&dsyoff=20px&dsblur=68px&wc=true&wa=true&pv=56px&ph=56px&ln=false&fl=1&fm=Hack&fs=14px&lh=133%25&si=false&es=2x&wm=false&code=using%2520System%253B%250A%250Aclass%2520Program%250A%257B%250A%2520%2520%2520%2520static%2520int%2520n%252C%2520pontElsoUresig%253B%250A%2520%2520%2520%2520static%2520int%255B%255D%2520pontok%2520%253D%2520new%2520int%255B0%255D%253B%250A%250A%2520%2520%2520%2520static%2520void%2520Main%28%29%250A%2520%2520%2520%2520%257B%250A%2520%2520%2520%2520%2520%2520%2520%2520%252F%252F%2520Be%253A%2520n%252C%2520pontok%250A%2520%2520%2520%2520%2520%2520%2520%2520n%2520%253D%25201%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520pontok%2520%253D%2520new%2520int%255B%255D%2520%257B%25200%252C%25200%2520%257D%253B%250A%250A%250A%2520%2520%2520%2520%2520%2520%2520%2520pontElsoUresig%2520%253D%25200%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520for%2520%28int%2520i%2520%253D%25201%253B%2520i%2520%253C%253D%2520elsoUres%28pontok%29%2520-%25201%253B%2520i%252B%252B%29%250A%2520%2520%2520%2520%2520%2520%2520%2520%257B%250A%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520pontElsoUresig%2520%253D%2520pontElsoUresig%2520%252B%2520pontok%255Bi%255D%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520%257D%250A%250A%2520%2520%2520%2520%2520%2520%2520%2520%252F%252F%2520Ki%253A%2520pontElsoUresig%250A%2520%2520%2520%2520%2520%2520%2520%2520Console.WriteLine%28%2522pontElsoUresig%2520%253D%2520%2522%2520%252B%2520pontElsoUresig%29%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520%252F%252F%2520V%25C3%25A1rt%2520%28teszteset%29%253A%2520pontElsoUresig%2520%253D%25200%250A%2520%2520%2520%2520%257D%250A%250A%2520%2520%2520%2520static%2520int%2520elsoUres%28int%255B%255D%2520pontok%29%250A%2520%2520%2520%2520%257B%250A%2520%2520%2520%2520%2520%2520%2520%2520bool%2520van%2520%253D%2520false%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520int%2520ind%2520%253D%25200%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520int%2520j%2520%253D%25200%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520j%2520%253D%25201%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520while%2520%28j%2520%253C%253D%2520n%2520%2526%2526%2520%21%28pontok%255Bj%255D%2520%253D%253D%25200%29%29%250A%2520%2520%2520%2520%2520%2520%2520%2520%257B%250A%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520j%2520%253D%2520j%2520%252B%25201%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520%257D%250A%2520%2520%2520%2520%2520%2520%2520%2520van%2520%253D%2520j%2520%253C%253D%2520n%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520if%2520%28van%29%250A%2520%2520%2520%2520%2520%2520%2520%2520%257B%250A%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520%2520ind%2520%253D%2520j%253B%250A%2520%2520%2520%2520%2520%2520%2520%2520%257D%250A%2520%2520%2520%2520%2520%2520%2520%2520return%2520van%2520%253F%2520ind%2520%253A%2520n%253B%250A%2520%2520%2520%2520%257D%250A%257D%250A";
        assert_eq!(carbon_link(code), expected);
    }

    /// A variable that gets its value first is declared with it, unless something needs it
    /// declared earlier.
    #[test]
    fn a_first_value_is_the_declaration() {
        use crate::stuki::model::{Diagram, Kind, Node, fresh_id};
        use crate::stuki::rich::Rich;
        let stmt = |t: &str| Node {
            id: fresh_id(),
            bg: None,
            kind: Kind::Statement(Rich::plain(t)),
        };
        let spec = parse("In: n∈Z\nOut: r∈Z\nPre: -\nPost: r = n\n").unwrap();
        let code = |vars: &str, body: &[&str]| {
            let mut d = Diagram::new();
            d.variables = Rich::plain(vars);
            d.body = body.iter().map(|t| stmt(t)).collect();
            let p = Program {
                diagrams: vec![d],
                ..Program::default()
            };
            let out = generate(&spec, &p, Inputs::Test, "n: 4\nr: 4\n").unwrap();
            out.code
        };
        // plain: declared with the value
        let c = code("i:Egész", &["i:=1", "r:=i+n"]);
        assert!(c.contains("int i = 1;") && !c.contains("int i = 0;"), "{c}");
        assert!(c.contains("int r = i + n;"), "{c}");
        // reads itself: starts with a value of its own, then the step
        let c = code("i:Egész", &["i:=i+1", "r:=i"]);
        assert!(c.contains("int i = 0;") && c.contains("i = i + 1;"), "{c}");
        // reads a variable that gets its value later: that one is declared first
        let c = code("a:Egész, b:Egész", &["a:=b+1", "b:=2", "r:=a+b"]);
        let (b0, a1) = (
            c.find("int b = 0;").expect(&c),
            c.find("int a = b + 1;").expect(&c),
        );
        assert!(
            b0 < a1 && c.contains("b = 2;") && !c.contains("int b = 2;"),
            "{c}"
        );
        // every step the routine starts with
        let c = code("i:Egész", &["r:=n", "i:=1"]);
        assert!(c.contains("int r = n;") && c.contains("int i = 1;"), "{c}");
        // but only those: after a decision a variable is declared first, as before
        let mut d = Diagram::new();
        d.variables = Rich::plain("i:Egész");
        d.body = vec![
            stmt("r:=n"),
            Node {
                id: fresh_id(),
                bg: None,
                kind: Kind::If {
                    cond: Rich::plain("n>0"),
                    yes: vec![stmt("r:=1")],
                    no: vec![Node::noop()],
                },
            },
            stmt("i:=1"),
        ];
        let p = Program {
            diagrams: vec![d],
            ..Program::default()
        };
        let c = generate(&spec, &p, Inputs::Test, "n: 4\nr: 4\n")
            .unwrap()
            .code;
        assert!(
            c.contains("int r = n;") && c.contains("int i = 0;") && c.contains("i = 1;"),
            "{c}"
        );
    }

    /// The expected output is what the console program prints (checked against dotnet's own
    /// output of these programs).
    #[test]
    fn console_output_is_what_the_program_prints() {
        for (name, expected) in [
            ("count", "3\n"),
            ("search", "2\n"),
            ("search_none", "-1\n"),
            ("max", "2\n9.25\n"),
            ("select", "3\n3 7 2 \n"),
            ("wind", "2\n"),
        ] {
            let (_, src, test) = cases().into_iter().find(|c| c.0 == name).unwrap();
            let (spec, _) = build(src);
            assert_eq!(console_output(&spec, test).unwrap(), expected, "{name}");
        }
    }

    #[test]
    fn console_input_matches_what_the_program_reads() {
        let (spec, _) = build(SEARCH);
        let text = console_input(
            &spec,
            "n: 4\nk: 10\nar: [30, 5, 20, 8]\nvan: true\nind: 2\nsorszam: 2\n",
        )
        .unwrap();
        assert_eq!(text, "4\n10\n30 5 20 8\n");
    }
}
