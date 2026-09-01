//! Canonical model: post-normalisation forms that define semantic
//! equality. See `dev_docs/0003-normalization.md` for the procedure.

mod normal;

pub use normal::{NormaliseError, NormaliseErrorKind, normalise_file};

/// A canonical constraint of the form `Σ coefficient · variable >= rhs`.
///
/// Terms are sorted lexicographically by variable name and every
/// coefficient is non-zero. The same canonical form is therefore a
/// stable hashable key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalConstraint {
    pub terms: Vec<(String, i64)>,
    pub rhs: i64,
}

/// A canonical objective: a sorted term list with no RHS, always in
/// minimisation form.
///
/// Constant terms in the source objective are dropped during
/// normalisation because shifting an objective by a constant does
/// not change what minimises it. A `max:` objective is negated, since
/// VeriPB minimises internally and negates one on load, so `max: f`
/// and `min: -f` share a canonical form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalObjective {
    pub terms: Vec<(String, i64)>,
}

/// A canonical `preserved:` list. Order does not matter for projected
/// enumeration, so the literals are sorted and de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalPreserved {
    /// `(variable, negated)` pairs, sorted, deduplicated.
    pub literals: Vec<(String, bool)>,
}

/// A canonical constraint together with the source-level metadata the
/// reporter needs (label, line number, original text).
///
/// One source line can stand for more than one constraint — an
/// equivalence (`<==>`) stands for two — in which case several of
/// these share a `line` and `raw` and are told apart by `part`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalLabelledConstraint {
    pub label: Option<String>,
    pub form: CanonicalConstraint,
    pub line: usize,
    pub raw: String,
    /// Which constraint of its source line this is, when the line
    /// stands for more than one. `None` for a line that stands for a
    /// single constraint, which is every line but an equivalence.
    pub part: Option<ConstraintPart>,
}

/// Which half of a source line standing for two constraints this is.
/// An equivalence is loaded as its `==>` direction followed by its
/// `<==` direction, and the i-th label on the line names the i-th of
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintPart {
    /// The `==>` direction of an equivalence.
    RightImplication,
    /// The `<==` direction of an equivalence.
    LeftImplication,
}

impl ConstraintPart {
    /// The arrow this part is written with, for reports.
    pub fn arrow(self) -> &'static str {
        match self {
            ConstraintPart::RightImplication => "==>",
            ConstraintPart::LeftImplication => "<==",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalObjectiveItem {
    pub form: CanonicalObjective,
    pub line: usize,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPreservedItem {
    pub form: CanonicalPreserved,
    pub line: usize,
    pub raw: String,
}

/// A complete file in canonical form. The comparison engine consumes
/// this directly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CanonicalFile {
    pub objective: Option<CanonicalObjectiveItem>,
    pub preserved: Option<CanonicalPreservedItem>,
    pub constraints: Vec<CanonicalLabelledConstraint>,
}
