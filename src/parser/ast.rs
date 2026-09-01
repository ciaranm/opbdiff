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
    /// Labels in source order. A line carries either no labels at all
    /// or one for each constraint it stands for, and the i-th label
    /// names the i-th constraint; see [`Constraint::constraint_count`].
    pub labels: Vec<String>,
    /// The reification shorthand this constraint is written with, if
    /// any. `None` for an ordinary constraint line.
    pub reification: Option<Reification>,
    pub terms: Vec<Term>,
    pub op: Op,
    pub rhs: i64,
    pub line: usize,
    pub raw: String,
}

impl Constraint {
    /// How many pseudo-Boolean constraints this line stands for: two
    /// for an equivalence (`<==>`), which is loaded as its `==>`
    /// direction followed by its `<==` direction, and one otherwise.
    pub fn constraint_count(&self) -> usize {
        match self.reification {
            Some(Reification::Equivalence(_)) => 2,
            _ => 1,
        }
    }
}

/// A reification shorthand: syntactic sugar for one or two ordinary
/// pseudo-Boolean constraints. See
/// `dev_docs/0003-normalization.md#reification-shorthands` for what
/// each one stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reification {
    /// `l1 … lm ==> C`: the conjunction of the literals implies the
    /// constraint. At least one literal.
    LiteralsImplyConstraint(Vec<Literal>),
    /// `z <== C`: the constraint implies the literal.
    ConstraintImpliesLiteral(Literal),
    /// `z <==> C`: both of the above, as two constraints.
    Equivalence(Literal),
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

impl Literal {
    /// This literal with its sign flipped: `x` becomes `~x` and `~x`
    /// becomes `x`. Named to avoid colliding with the `negated` field.
    pub fn flipped(&self) -> Literal {
        Literal {
            variable: self.variable.clone(),
            negated: !self.negated,
        }
    }
}
