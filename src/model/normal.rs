//! AST → canonical-form normalisation.
//!
//! Implements the procedure in `dev_docs/0003-normalization.md`. All
//! arithmetic is checked; overflow surfaces as a `NormaliseError`
//! rather than a panic, even though real OPB files are nowhere near
//! the `i64` boundary.

use std::collections::BTreeMap;

use crate::parser::{
    Constraint, Item, Literal, Objective, Op, OpbFile, Preserved, Reification, Term,
};

use super::{
    CanonicalConstraint, CanonicalFile, CanonicalLabelledConstraint, CanonicalObjective,
    CanonicalObjectiveItem, CanonicalPreserved, CanonicalPreservedItem, ConstraintPart,
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
                // A line can stand for more than one constraint: an
                // equivalence is loaded as its `==>` direction followed
                // by its `<==` direction, and the i-th label names the
                // i-th of them.
                let forms = normalise_constraint(c)?;
                let parts: &[Option<ConstraintPart>] = if forms.len() > 1 {
                    &[
                        Some(ConstraintPart::RightImplication),
                        Some(ConstraintPart::LeftImplication),
                    ]
                } else {
                    &[None]
                };
                for (index, form) in forms.into_iter().enumerate() {
                    out.constraints.push(CanonicalLabelledConstraint {
                        label: c.labels.get(index).cloned(),
                        form,
                        line: c.line,
                        raw: c.raw.clone(),
                        part: parts[index],
                    });
                }
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

/// Normalise one constraint line into the canonical constraints it
/// stands for: one ordinarily, two for an equivalence (`<==>`), in the
/// order VeriPB loads them.
fn normalise_constraint(c: &Constraint) -> Result<Vec<CanonicalConstraint>, NormaliseError> {
    let overflow = || NormaliseError {
        line: c.line,
        kind: NormaliseErrorKind::Overflow,
    };
    let base = normalise_plain_constraint(c)?;
    let forms = match &c.reification {
        None => vec![base],
        Some(Reification::LiteralsImplyConstraint(literals)) => {
            vec![right_implication(base, literals).map_err(|()| overflow())?]
        }
        Some(Reification::ConstraintImpliesLiteral(literal)) => {
            vec![left_implication(base, literal).map_err(|()| overflow())?]
        }
        // An equivalence is both implications, in the order VeriPB
        // loads them, over the *same* base constraint.
        Some(Reification::Equivalence(literal)) => vec![
            right_implication(base.clone(), std::slice::from_ref(literal))
                .map_err(|()| overflow())?,
            left_implication(base, literal).map_err(|()| overflow())?,
        ],
    };
    Ok(forms)
}

/// Normalise the constraint proper, ignoring any reification shorthand
/// it is written with.
fn normalise_plain_constraint(c: &Constraint) -> Result<CanonicalConstraint, NormaliseError> {
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

/// The degree of `form` as VeriPB normalises it: with every coefficient
/// made positive by rewriting `-a · x` as `a · ~x - a`, which moves `a`
/// to the right-hand side.
///
/// This is the coefficient a reification carries its literals at, so it
/// has to be read off the *normalised* constraint — terms that cancel
/// each other change it, and a reification built on an un-normalised
/// degree would be too strong.
fn normalised_degree(form: &CanonicalConstraint) -> Result<i64, ()> {
    let mut degree = form.rhs;
    for (_, coefficient) in &form.terms {
        if *coefficient < 0 {
            degree = degree.checked_sub(*coefficient).ok_or(())?;
        }
    }
    Ok(degree)
}

/// The sum of the absolute values of the coefficients: VeriPB's
/// `coeff_sum`, which bounds what the left-hand side can reach.
fn coefficient_sum(form: &CanonicalConstraint) -> Result<i64, ()> {
    let mut sum: i64 = 0;
    for (_, coefficient) in &form.terms {
        sum = sum
            .checked_add(coefficient.checked_abs().ok_or(())?)
            .ok_or(())?;
    }
    Ok(sum)
}

/// `l1 … lm ==> C`: the conjunction of the literals implies `C`.
///
/// Each literal is negated and added to `C` with `C`'s degree as
/// coefficient, so falsifying any one of them satisfies the constraint
/// on its own and the rest of it says nothing. A `C` that is trivially
/// satisfied — degree not positive — is implied by anything and keeps
/// none of the literals: adding them at a degree that is not positive
/// would give a constraint that is too strong.
fn right_implication(
    base: CanonicalConstraint,
    literals: &[Literal],
) -> Result<CanonicalConstraint, ()> {
    let degree = normalised_degree(&base)?;
    if degree <= 0 {
        return Ok(base);
    }
    let mut rhs = base.rhs;
    let mut terms: BTreeMap<String, i64> = base.terms.into_iter().collect();
    for literal in literals {
        add_literal(&mut terms, &mut rhs, degree, &literal.flipped())?;
    }
    Ok(rebuild(terms, rhs))
}

/// `z <== C`: `C` implies the literal `z`.
///
/// Here it is `C` that is negated, and `z` is added to that negation
/// with the negation's degree as coefficient. A contradicting `C` —
/// degree larger than the sum of its coefficients — implies anything,
/// so its (trivially satisfied) negation is the whole of what it
/// stands for.
fn left_implication(
    base: CanonicalConstraint,
    literal: &Literal,
) -> Result<CanonicalConstraint, ()> {
    let degree = normalised_degree(&base)?;
    let sum = coefficient_sum(&base)?;

    // ¬C: `Σ k·v >= r` is false exactly when `Σ (-k)·v >= 1 - r`.
    let mut rhs = 1_i64.checked_sub(base.rhs).ok_or(())?;
    let mut terms: BTreeMap<String, i64> = BTreeMap::new();
    for (var, coefficient) in base.terms {
        terms.insert(var, coefficient.checked_neg().ok_or(())?);
    }

    if degree > sum {
        return Ok(rebuild(terms, rhs));
    }

    // The degree of ¬C, which is what `z` is carried at.
    let negated_degree = sum
        .checked_sub(degree)
        .and_then(|d| d.checked_add(1))
        .ok_or(())?;
    add_literal(&mut terms, &mut rhs, negated_degree, literal)?;
    Ok(rebuild(terms, rhs))
}

/// Add `coefficient · literal` to a canonical constraint's left-hand
/// side. A negated literal is rewritten as `c · ~x = (-c) · x + c` and
/// the constant moves to the right-hand side, exactly as
/// [`collect_linear_form`] does for source terms.
fn add_literal(
    terms: &mut BTreeMap<String, i64>,
    rhs: &mut i64,
    coefficient: i64,
    literal: &Literal,
) -> Result<(), ()> {
    if literal.negated {
        add_to_var(
            terms,
            &literal.variable,
            coefficient.checked_neg().ok_or(())?,
        )?;
        *rhs = rhs.checked_sub(coefficient).ok_or(())?;
    } else {
        add_to_var(terms, &literal.variable, coefficient)?;
    }
    Ok(())
}

/// Rebuild a canonical constraint from a term map, dropping terms whose
/// coefficient cancelled to zero. A reification literal can share a
/// variable with the constraint it reifies, so this re-runs the last
/// steps of normalisation over the combined terms.
fn rebuild(terms: BTreeMap<String, i64>, rhs: i64) -> CanonicalConstraint {
    CanonicalConstraint {
        terms: terms.into_iter().filter(|(_, k)| *k != 0).collect(),
        rhs,
    }
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

    /// Every canonical constraint a source snippet stands for.
    fn all_constraints(input: &str) -> Vec<CanonicalConstraint> {
        let f = parse(input).expect("parse");
        let n = normalise_file(&f).expect("normalise");
        n.constraints.into_iter().map(|c| c.form).collect()
    }

    /// Assert that a reification shorthand normalises to exactly what
    /// the explicit constraints do.
    fn same(sugar: &str, explicit: &str) {
        assert_eq!(
            all_constraints(sugar),
            all_constraints(explicit),
            "\n  sugar:    {sugar}  explicit: {explicit}"
        );
    }

    // The reification examples below are the worked ones from VeriPB's
    // `proof_format_overview.md`, which is what defines the sugar.

    #[test]
    fn right_implication_matches_veripb_expansion() {
        same(
            "z1 z2 ~z3 ==> +1 x1 +2 x2 >= 2 ;\n",
            "2 ~z1 2 ~z2 2 z3 1 x1 2 x2 >= 2 ;\n",
        );
    }

    #[test]
    fn left_implication_matches_veripb_expansion() {
        same("z1 <== +1 x1 +2 x2 >= 2 ;\n", "2 z1 1 ~x1 2 ~x2 >= 2 ;\n");
    }

    #[test]
    fn trivially_satisfied_right_implication_loses_its_literals() {
        // A constraint whose degree is not positive is implied by
        // anything, so it is loaded as it is. Carrying the literals at a
        // degree that is not positive would give a constraint that is
        // too strong.
        same("z1 z2 ==> 1 x1 >= -1 ;\n", "1 x1 >= -1 ;\n");
    }

    #[test]
    fn contradicting_left_implication_loses_its_literal() {
        // A contradicting constraint implies anything, so only its
        // (trivially satisfied) negation is loaded.
        same("z1 <== 1 x1 >= 5 ;\n", "1 ~x1 >= -3 ;\n");
    }

    #[test]
    fn equivalence_is_both_directions_in_order() {
        same(
            "z1 <==> 1 x1 1 x2 >= 1 ;\n",
            "1 ~z1 1 x1 1 x2 >= 1 ;\n2 z1 1 ~x1 1 ~x2 >= 2 ;\n",
        );
    }

    #[test]
    fn equivalence_labels_name_the_two_directions_in_order() {
        let f = parse("@right @left z1 <==> 1 x1 1 x2 >= 1 ;\n").unwrap();
        let n = normalise_file(&f).unwrap();
        assert_eq!(n.constraints.len(), 2);
        assert_eq!(n.constraints[0].label.as_deref(), Some("right"));
        assert_eq!(
            n.constraints[0].part,
            Some(ConstraintPart::RightImplication)
        );
        assert_eq!(n.constraints[1].label.as_deref(), Some("left"));
        assert_eq!(n.constraints[1].part, Some(ConstraintPart::LeftImplication));
        // Both halves come from the same source line.
        assert_eq!(n.constraints[0].line, n.constraints[1].line);
        assert_eq!(n.constraints[0].raw, n.constraints[1].raw);
    }

    #[test]
    fn an_ordinary_constraint_has_no_part() {
        let f = parse("1 x1 >= 1 ;\n").unwrap();
        let n = normalise_file(&f).unwrap();
        assert_eq!(n.constraints[0].part, None);
    }

    #[test]
    fn reification_degree_is_taken_after_cancellation() {
        // The literals are carried at the degree, so the degree has to
        // be the one the constraint has *after* normalisation: here
        // `1 x2 1 ~x2` cancels to the constant 1, leaving degree 1, not
        // the 2 that is written.
        same("z1 ==> 1 x2 1 ~x2 1 x1 >= 2 ;\n", "1 ~z1 1 x1 >= 1 ;\n");
    }

    #[test]
    fn reification_literal_may_share_a_variable_with_the_constraint() {
        // `x1 ==> 1 x1 1 x2 >= 2` adds `2 ~x1` to the constraint, which
        // merges with the `1 x1` already there.
        same("x1 ==> 1 x1 1 x2 >= 2 ;\n", "-1 x1 1 x2 >= 0 ;\n");
    }

    #[test]
    fn right_implication_of_a_le_constraint() {
        // (z1 ∧ z2) → ¬x1, i.e. not all three of x1, z1, z2.
        same("z1 z2 ==> 1 x1 <= 0 ;\n", "1 x1 1 z1 1 z2 <= 2 ;\n");
    }

    #[test]
    fn left_implication_of_a_le_constraint() {
        // (x1 ≤ 0) → z1, i.e. x1 ∨ z1.
        same("z1 <== 1 x1 <= 0 ;\n", "1 x1 1 z1 >= 1 ;\n");
    }

    #[test]
    fn reification_on_a_negated_literal() {
        // The antecedent literal is negated before it is added, so a
        // `~z` antecedent contributes a positive `z` term.
        same("~z1 ==> 1 x1 >= 1 ;\n", "1 z1 1 x1 >= 1 ;\n");
    }

    #[test]
    fn a_repeated_antecedent_literal_is_added_twice() {
        // Nothing de-duplicates the antecedent, in VeriPB or here: the
        // literal is added once per occurrence and the terms merge.
        same("x1 x1 ==> 1 x2 >= 1 ;\n", "2 ~x1 1 x2 >= 1 ;\n");
    }

    #[test]
    fn a_contradictory_antecedent_leaves_a_vacuous_constraint() {
        // `x1 and not x1` is never true, so the implication says
        // nothing: the two added terms cancel and take the degree with
        // them.
        same("x1 ~x1 ==> 1 x2 >= 1 ;\n", "1 x2 >= 0 ;\n");
    }

    #[test]
    fn a_reified_constraint_is_not_equal_to_the_bare_one() {
        // Guard against desugaring silently dropping the reification.
        assert_ne!(
            all_constraints("z1 ==> 1 x1 >= 1 ;\n"),
            all_constraints("1 x1 >= 1 ;\n"),
        );
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
            labels: Vec::new(),
            reification: None,
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
