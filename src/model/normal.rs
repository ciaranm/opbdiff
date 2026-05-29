//! AST → canonical-form normalisation.
//!
//! Implements the procedure in `dev_docs/0003-normalization.md`. All
//! arithmetic is checked; overflow surfaces as a `NormaliseError`
//! rather than a panic, even though real OPB files are nowhere near
//! the `i64` boundary.

use std::collections::BTreeMap;

use crate::parser::{Constraint, Item, Objective, Op, OpbFile, Preserved, Term};

use super::{
    CanonicalConstraint, CanonicalFile, CanonicalLabelledConstraint, CanonicalObjective,
    CanonicalObjectiveItem, CanonicalPreserved, CanonicalPreservedItem,
};

#[derive(Debug, Clone, thiserror::Error)]
#[error("line {line}: {kind}")]
pub struct NormaliseError {
    pub line: usize,
    pub kind: NormaliseErrorKind,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum NormaliseErrorKind {
    #[error("integer overflow during normalisation")]
    Overflow,
    #[error("more than one `min:` objective in file")]
    MultipleObjectives,
    #[error("more than one `preserved:` line in file")]
    MultiplePreserved,
}

/// Normalise every item in a parsed OPB file. Errors include
/// duplicate `min:` / `preserved:` lines and arithmetic overflow.
pub fn normalise_file(file: &OpbFile) -> Result<CanonicalFile, NormaliseError> {
    let mut out = CanonicalFile::default();
    for item in &file.items {
        match item {
            Item::Constraint(c) => {
                let form = normalise_constraint(c)?;
                out.constraints.push(CanonicalLabelledConstraint {
                    label: c.label.clone(),
                    form,
                    line: c.line,
                    raw: c.raw.clone(),
                });
            }
            Item::Objective(o) => {
                if out.objective.is_some() {
                    return Err(NormaliseError {
                        line: o.line,
                        kind: NormaliseErrorKind::MultipleObjectives,
                    });
                }
                out.objective = Some(CanonicalObjectiveItem {
                    form: normalise_objective(o)?,
                    line: o.line,
                    raw: o.raw.clone(),
                });
            }
            Item::Preserved(p) => {
                if out.preserved.is_some() {
                    return Err(NormaliseError {
                        line: p.line,
                        kind: NormaliseErrorKind::MultiplePreserved,
                    });
                }
                out.preserved = Some(CanonicalPreservedItem {
                    form: normalise_preserved(p),
                    line: p.line,
                    raw: p.raw.clone(),
                });
            }
        }
    }
    Ok(out)
}

fn normalise_constraint(c: &Constraint) -> Result<CanonicalConstraint, NormaliseError> {
    let overflow = || NormaliseError {
        line: c.line,
        kind: NormaliseErrorKind::Overflow,
    };

    // Step 1: orient the inequality to `>=`. If the source uses `<=`,
    // negate everything (coefficients via the `sign` factor below, and
    // the RHS here).
    let (sign, rhs) = match c.op {
        Op::GreaterOrEqual => (1_i64, c.rhs),
        Op::LessOrEqual => (-1_i64, c.rhs.checked_neg().ok_or_else(overflow)?),
    };

    let (var_coefs, lhs_constant_sum) =
        collect_linear_form(&c.terms, sign).map_err(|()| overflow())?;

    // Step 3: move LHS constants to the RHS: rhs := rhs - lhs_constants.
    let new_rhs = rhs.checked_sub(lhs_constant_sum).ok_or_else(overflow)?;

    // Step 5 + 6: drop zero-coefficient terms; BTreeMap iteration is
    // already lexicographically ordered.
    let terms: Vec<(String, i64)> = var_coefs.into_iter().filter(|(_, k)| *k != 0).collect();

    Ok(CanonicalConstraint {
        terms,
        rhs: new_rhs,
    })
}

fn normalise_objective(o: &Objective) -> Result<CanonicalObjective, NormaliseError> {
    let overflow = || NormaliseError {
        line: o.line,
        kind: NormaliseErrorKind::Overflow,
    };

    // Objectives have no operator and no RHS. Any constant terms
    // accumulated by the negated-literal rewrite are dropped, because
    // shifting an objective by a constant does not change what
    // minimises it.
    let (var_coefs, _constant_sum) = collect_linear_form(&o.terms, 1).map_err(|()| overflow())?;

    let terms: Vec<(String, i64)> = var_coefs.into_iter().filter(|(_, k)| *k != 0).collect();

    Ok(CanonicalObjective { terms })
}

fn normalise_preserved(p: &Preserved) -> CanonicalPreserved {
    let mut literals: Vec<(String, bool)> = p
        .literals
        .iter()
        .map(|l| (l.variable.clone(), l.negated))
        .collect();
    literals.sort();
    literals.dedup();
    CanonicalPreserved { literals }
}

/// Walk a list of terms, multiplying every coefficient by `sign`,
/// rewriting `c · ~x` as `(-c) · x + c`, summing like terms into a
/// `BTreeMap` keyed by variable name, and accumulating LHS constants
/// separately.
///
/// Returns `(var_coefficients_sorted_by_var, lhs_constant_sum)`. The
/// `()` error means overflow during checked arithmetic.
fn collect_linear_form(terms: &[Term], sign: i64) -> Result<(BTreeMap<String, i64>, i64), ()> {
    let mut var_coefs: BTreeMap<String, i64> = BTreeMap::new();
    let mut lhs_constant_sum: i64 = 0;

    for term in terms {
        match term {
            Term::Linear {
                coefficient,
                literal,
            } => {
                let signed = coefficient.checked_mul(sign).ok_or(())?;
                if literal.negated {
                    // signed · ~x = (-signed) · x + signed
                    let neg = signed.checked_neg().ok_or(())?;
                    add_to_var(&mut var_coefs, &literal.variable, neg)?;
                    lhs_constant_sum = lhs_constant_sum.checked_add(signed).ok_or(())?;
                } else {
                    add_to_var(&mut var_coefs, &literal.variable, signed)?;
                }
            }
            Term::Constant(k) => {
                let signed = k.checked_mul(sign).ok_or(())?;
                lhs_constant_sum = lhs_constant_sum.checked_add(signed).ok_or(())?;
            }
        }
    }

    Ok((var_coefs, lhs_constant_sum))
}

fn add_to_var(map: &mut BTreeMap<String, i64>, var: &str, delta: i64) -> Result<(), ()> {
    let entry = map.entry(var.to_owned()).or_insert(0);
    *entry = entry.checked_add(delta).ok_or(())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn one_constraint(input: &str) -> CanonicalConstraint {
        let f = parse(input).expect("parse");
        let n = normalise_file(&f).expect("normalise");
        assert_eq!(n.constraints.len(), 1);
        n.constraints.into_iter().next().unwrap().form
    }

    #[test]
    fn worked_example_from_dev_docs_matches() {
        // The two lines that motivate the whole tool.
        let a = one_constraint("1 x1 1 x2 1 x3 >= 2 ;\n");
        let b = one_constraint("+1 ~x3 +1 ~x2 +1 ~x1 <= 1 ;\n");
        assert_eq!(a, b);
        assert_eq!(
            a,
            CanonicalConstraint {
                terms: vec![("x1".into(), 1), ("x2".into(), 1), ("x3".into(), 1),],
                rhs: 2,
            }
        );
    }

    #[test]
    fn flips_le_to_ge() {
        // 1 x1 <= 3 normalises to -1 x1 >= -3.
        let c = one_constraint("1 x1 <= 3 ;\n");
        assert_eq!(c.terms, vec![("x1".into(), -1)]);
        assert_eq!(c.rhs, -3);
    }

    #[test]
    fn moves_lhs_constants_to_rhs() {
        // 1 x1 - 2 >= 0  ==>  1 x1 >= 2
        let c = one_constraint("1 x1 -2 >= 0 ;\n");
        assert_eq!(c.terms, vec![("x1".into(), 1)]);
        assert_eq!(c.rhs, 2);
    }

    #[test]
    fn combines_like_terms() {
        // 1 x1 + 2 x1 >= 1  ==>  3 x1 >= 1
        let c = one_constraint("1 x1 2 x1 >= 1 ;\n");
        assert_eq!(c.terms, vec![("x1".into(), 3)]);
        assert_eq!(c.rhs, 1);
    }

    #[test]
    fn x_plus_not_x_collapses_to_constant() {
        // 1 x1 + 1 ~x1 >= 1  ==>  (sum of x1 coefs = 0; LHS constant = +1)
        //                         0 >= 1 - 1  ==>  0 >= 0, terms empty, rhs 0.
        let c = one_constraint("1 x1 1 ~x1 >= 1 ;\n");
        assert!(c.terms.is_empty());
        assert_eq!(c.rhs, 0);
    }

    #[test]
    fn drops_zero_coefficient_terms() {
        // 1 x1 + 0 x2 >= 1  ==>  1 x1 >= 1, x2 dropped.
        let c = one_constraint("1 x1 0 x2 >= 1 ;\n");
        assert_eq!(c.terms, vec![("x1".into(), 1)]);
    }

    #[test]
    fn terms_are_sorted_by_variable_name() {
        let c = one_constraint("1 z 1 a 1 m >= 0 ;\n");
        let names: Vec<&str> = c.terms.iter().map(|(v, _)| v.as_str()).collect();
        assert_eq!(names, vec!["a", "m", "z"]);
    }

    #[test]
    fn coefficient_sign_does_not_get_normalised_away() {
        // Important: we do not flip "all-negative" to "all-positive" by
        // multiplying through by -1; that would change the meaning.
        let c = one_constraint("-1 x1 >= -2 ;\n");
        assert_eq!(c.terms, vec![("x1".into(), -1)]);
        assert_eq!(c.rhs, -2);
    }

    #[test]
    fn empty_objective_after_cancellation() {
        // min: 1 x1 + 1 ~x1  ==>  the variable terms cancel and the
        // implied constant is dropped; canonical objective has no terms.
        let f = parse("min: 1 x1 1 ~x1 ;\n").unwrap();
        let n = normalise_file(&f).unwrap();
        let obj = n.objective.unwrap();
        assert!(obj.form.terms.is_empty());
    }

    #[test]
    fn preserved_is_sorted_and_deduplicated() {
        let f = parse("preserved: x3 x1 x2 x1 ;\n").unwrap();
        let n = normalise_file(&f).unwrap();
        let pres = n.preserved.unwrap();
        let names: Vec<(&str, bool)> = pres
            .form
            .literals
            .iter()
            .map(|(v, n)| (v.as_str(), *n))
            .collect();
        assert_eq!(names, vec![("x1", false), ("x2", false), ("x3", false)]);
    }

    #[test]
    fn multiple_objectives_rejected() {
        let input = "min: 1 x1 ;\nmin: 1 x2 ;\n";
        let f = parse(input).unwrap();
        let err = normalise_file(&f).unwrap_err();
        assert!(matches!(err.kind, NormaliseErrorKind::MultipleObjectives));
        assert_eq!(err.line, 2);
    }

    #[test]
    fn multiple_preserved_rejected() {
        let input = "preserved: x1 ;\npreserved: x2 ;\n";
        let f = parse(input).unwrap();
        let err = normalise_file(&f).unwrap_err();
        assert!(matches!(err.kind, NormaliseErrorKind::MultiplePreserved));
        assert_eq!(err.line, 2);
    }

    #[test]
    fn overflow_on_negate_is_reported_not_panic() {
        // i64::MIN cannot be negated. Force this through a `<=` flip on a
        // constraint whose RHS is exactly i64::MIN. Lexer accepts arbitrary
        // i64 input, so we hand-build the constraint AST is more direct
        // but we can also exercise via parse + a tiny manual constraint.
        use crate::parser::{Constraint, Op as POp, OpbFile};
        let bad = Constraint {
            label: None,
            terms: vec![],
            op: POp::LessOrEqual,
            rhs: i64::MIN,
            line: 1,
            raw: "synthetic".into(),
        };
        let file = OpbFile {
            items: vec![Item::Constraint(bad)],
        };
        let err = normalise_file(&file).unwrap_err();
        assert!(matches!(err.kind, NormaliseErrorKind::Overflow));
    }

    #[test]
    fn realworld_pair_first_constraint_matches() {
        // From odd_even_sum.opb line 3 vs odd_even_sum.verifiedopb line 2:
        //   .opb         :        1 i[a][b0] 2 i[a][b1] 4 i[a][b2] >= 0
        //   .verifiedopb : @i[a][lb] 1 i[a][b0] 2 i[a][b1] 4 i[a][b2] >= 0
        let a = one_constraint("1 i[a][b0] 2 i[a][b1] 4 i[a][b2] >= 0 ;\n");
        let b = one_constraint("@i[a][lb] 1 i[a][b0] 2 i[a][b1] 4 i[a][b2] >= 0 ;\n");
        assert_eq!(a, b, "label does not affect canonical form");
    }

    #[test]
    fn realworld_pair_upper_bound_matches() {
        // .opb         :        -1 i[a][b0] -2 i[a][b1] -4 i[a][b2] >= -5
        // .verifiedopb : @i[a][ub] 1 i[a][b0]  2 i[a][b1]  4 i[a][b2] <= 5
        let a = one_constraint("-1 i[a][b0] -2 i[a][b1] -4 i[a][b2] >= -5 ;\n");
        let b = one_constraint("@i[a][ub] 1 i[a][b0] 2 i[a][b1] 4 i[a][b2] <= 5 ;\n");
        assert_eq!(a, b);
    }
}
