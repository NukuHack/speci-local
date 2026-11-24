//! How the console program of the Code tab lays out its input and output: which values share a
//! line and which are split over several. The choice is made per variable ([`VarLayout`]); this
//! module only holds the choices and works out what they mean ([`plan`]). The C# generator
//! ([`crate::csharp`]) writes both the program and the text it reads and prints from the plan, so
//! the two always agree.
//!
//! A variable's value is a tree: a table holds rows, a row holds items, a record holds fields.
//! The [`Level`] says how deep the line breaks go:
//!
//! - [`Level::Single`]: the whole value in one line (`1 2 3 4 5`).
//! - [`Level::Rows`]: one line per row of a table, or per record of a list of records.
//! - [`Level::Each`]: every number (every field of a record) in a line of its own.
//!
//! Values may also share a line: a variable that is [`Entry::join`]ed continues the line of the
//! one before it (`n m` in one line). That works when the one before has a fixed width (a number
//! or a record) and this one fits in a line; [`plan`] leaves out a join that does not.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum Level {
    /// What the variable's kind does by default (see [`Kind::default_level`]).
    #[default]
    Auto,
    Single,
    Rows,
    Each,
}

/// The shape of a variable, as far as the line layout is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A number, a truth value or a character: one token.
    Scalar,
    /// A text: the whole line it is in.
    Text,
    /// A record of simple values.
    Record,
    /// A one-dimensional array of simple values.
    List,
    /// A one-dimensional array of records.
    Records,
    /// A two-dimensional array.
    Table,
    /// More dimensions than the program reads or prints.
    Other,
}

impl Kind {
    /// The levels the variable can be laid out in, the default one first.
    pub fn choices(self, input: bool) -> &'static [Level] {
        match (self, input) {
            (Kind::Record | Kind::List, _) => &[Level::Single, Level::Each],
            (Kind::Records, true) => &[Level::Rows, Level::Single, Level::Each],
            (Kind::Records, false) => &[Level::Single, Level::Rows, Level::Each],
            (Kind::Table, _) => &[Level::Rows, Level::Single, Level::Each],
            _ => &[],
        }
    }

    /// What `Level::Auto` is: the layout the program had before layouts could be chosen.
    pub fn default_level(self, input: bool) -> Level {
        self.choices(input)
            .first()
            .copied()
            .unwrap_or(Level::Single)
    }

    /// The level that is really used for `want`: the default one if the kind has no such choice.
    pub fn resolve(self, input: bool, want: Level) -> Level {
        if self.choices(input).contains(&want) {
            want
        } else {
            self.default_level(input)
        }
    }

    /// The same number of tokens every time (so that more can follow in the line).
    fn fixed_width(self, level: Level) -> bool {
        matches!(self, Kind::Scalar | Kind::Record) && level == Level::Single
    }

    /// Fits in one line, next to others.
    fn one_line(self, level: Level) -> bool {
        !matches!(self, Kind::Text | Kind::Other) && level == Level::Single
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VarLayout {
    pub level: Level,
    /// Continue the line of the variable before this one.
    pub join: bool,
}

/// The layout choices of a project, by variable name. A variable that is not in it has the
/// default layout.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IoLayout {
    vars: BTreeMap<String, VarLayout>,
}

impl IoLayout {
    pub fn get(&self, name: &str) -> VarLayout {
        self.vars.get(name).copied().unwrap_or_default()
    }

    fn edit(&mut self, name: &str, f: impl FnOnce(&mut VarLayout)) {
        let mut v = self.get(name);
        f(&mut v);
        if v == VarLayout::default() {
            self.vars.remove(name);
        } else {
            self.vars.insert(name.to_string(), v);
        }
    }

    pub fn set_level(&mut self, name: &str, level: Level) {
        self.edit(name, |v| v.level = level);
    }

    pub fn set_join(&mut self, name: &str, join: bool) {
        self.edit(name, |v| v.join = join);
    }

    /// Nothing was changed from the default layout.
    pub fn is_default(&self) -> bool {
        self.vars.is_empty()
    }

    pub fn reset(&mut self) {
        self.vars.clear();
    }
}

/// What the layout means for one variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    /// The level in use (never `Auto`).
    pub level: Level,
    /// The variable continues the line of the one before it.
    pub join: bool,
    /// It could: the choice is offered (the one before has a fixed width, this one fits in a line).
    pub can_join: bool,
}

/// The layout of a list of variables (the inputs, or the outputs, in the order of the
/// specification). A join that is not possible is left out.
pub fn plan(vars: &[(String, Kind)], layout: &IoLayout, input: bool) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(vars.len());
    for (name, kind) in vars {
        let want = layout.get(name);
        let level = kind.resolve(input, want.level);
        let can_join = out
            .last()
            .is_some_and(|p| p.kind.fixed_width(p.level) && kind.one_line(level));
        out.push(Entry {
            name: name.clone(),
            kind: *kind,
            level,
            join: can_join && want.join,
            can_join,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vec<(String, Kind)> {
        [
            ("n", Kind::Scalar),
            ("m", Kind::Scalar),
            ("s", Kind::Text),
            ("t", Kind::List),
            ("u", Kind::Table),
        ]
        .map(|(n, k)| (n.to_string(), k))
        .to_vec()
    }

    #[test]
    fn nothing_chosen_is_the_default() {
        let p = plan(&vars(), &IoLayout::default(), true);
        assert_eq!(p[3].level, Level::Single);
        assert_eq!(p[4].level, Level::Rows);
        assert!(p.iter().all(|e| !e.join));
    }

    #[test]
    fn a_level_the_kind_does_not_have_is_the_default() {
        let mut l = IoLayout::default();
        l.set_level("n", Level::Each);
        l.set_level("t", Level::Rows);
        let p = plan(&vars(), &l, true);
        assert_eq!(p[0].level, Level::Single);
        assert_eq!(p[3].level, Level::Single);
    }

    #[test]
    fn only_a_fixed_width_value_can_have_more_in_its_line() {
        let mut l = IoLayout::default();
        for n in ["m", "s", "t", "u"] {
            l.set_join(n, true);
        }
        let p = plan(&vars(), &l, true);
        assert!(p[1].join, "m after n");
        assert!(!p[2].join && !p[2].can_join, "a text takes its line");
        assert!(!p[3].join && !p[3].can_join, "nothing joins a text");
        assert!(!p[4].can_join, "a table has more than one line");
        l.set_level("t", Level::Single);
        assert!(plan(&vars(), &l, true)[1].join);
    }

    #[test]
    fn an_array_ends_its_line() {
        let v = vec![
            ("t".to_string(), Kind::List),
            ("n".to_string(), Kind::Scalar),
        ];
        let mut l = IoLayout::default();
        l.set_join("n", true);
        assert!(!plan(&v, &l, false)[1].join);
        let v = vec![
            ("n".to_string(), Kind::Scalar),
            ("t".to_string(), Kind::List),
        ];
        let mut l = IoLayout::default();
        l.set_join("t", true);
        assert!(plan(&v, &l, false)[1].join, "a list may end the line");
    }

    #[test]
    fn records_differ_between_input_and_output() {
        assert_eq!(Kind::Records.default_level(true), Level::Rows);
        assert_eq!(Kind::Records.default_level(false), Level::Single);
    }

    #[test]
    fn choosing_the_default_again_forgets_the_choice() {
        let mut l = IoLayout::default();
        l.set_level("t", Level::Each);
        assert!(!l.is_default());
        l.set_level("t", Level::Auto);
        assert!(l.is_default());
    }
}
