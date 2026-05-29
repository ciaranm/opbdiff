//! Parser AST. Faithful to source: keeps original literals, operator,
//! RHS sign, and source-line numbers so the reporter can show what
//! the user actually wrote. Normalisation lives in `crate::model`.

/// A parsed OPB file: a sequence of items in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpbFile {
    pub items: Vec<Item>,
}

/// One top-level entry from an OPB file. Comments are dropped during
/// parsing and never appear in the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Constraint(Constraint),
    Objective(Objective),
    Preserved(Preserved),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    pub label: Option<String>,
    pub terms: Vec<Term>,
    pub op: Op,
    pub rhs: i64,
    pub line: usize,
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    GreaterOrEqual,
    LessOrEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    pub terms: Vec<Term>,
    pub line: usize,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preserved {
    pub literals: Vec<Literal>,
    pub line: usize,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Linear { coefficient: i64, literal: Literal },
    Constant(i64),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Literal {
    pub variable: String,
    pub negated: bool,
}
