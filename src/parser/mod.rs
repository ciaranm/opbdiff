//! OPB parser. Line-oriented, building an AST that preserves source
//! information for the reporter. Comments and blank lines are dropped.

mod ast;
mod lexer;

pub use ast::{Constraint, Item, Literal, Objective, Op, OpbFile, Preserved, Reification, Term};

use lexer::{Token, tokenize_line};

/// Parse a complete OPB file.
///
/// On the first hard parse error, returns `Err` with a line number and a
/// kind describing what went wrong. Comments (`*`) and blank lines are
/// ignored.
pub fn parse(input: &str) -> Result<OpbFile, ParseError> {
    let mut items = Vec::new();
    for (idx, raw_line) in input.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('*') {
            continue;
        }
        if let Some(item) = parse_line(raw_line, line_no)? {
            items.push(item);
        }
    }
    Ok(OpbFile { items })
}

fn parse_line(raw: &str, line: usize) -> Result<Option<Item>, ParseError> {
    let tokens = tokenize_line(raw);
    if tokens.is_empty() {
        return Ok(None);
    }
    // A constraint line may carry several labels: one for each
    // constraint the line stands for. Collect them all before deciding
    // what kind of line this is.
    let mut labels = Vec::new();
    let mut rest = &tokens[..];
    while let Some(Token::Label(name)) = rest.first() {
        labels.push((*name).to_owned());
        rest = &rest[1..];
    }
    match rest.first() {
        // Only a constraint can carry a label, so `@l min: ...` is an
        // error rather than an objective.
        Some(Token::Min) if labels.is_empty() => {
            parse_objective(&rest[1..], raw, line, false).map(Some)
        }
        Some(Token::Max) if labels.is_empty() => {
            parse_objective(&rest[1..], raw, line, true).map(Some)
        }
        Some(Token::Preserved) if labels.is_empty() => {
            parse_preserved(&rest[1..], raw, line).map(Some)
        }
        Some(Token::Min | Token::Max | Token::Preserved) => Err(ParseError {
            line,
            kind: ParseErrorKind::LabelsOnNonConstraintLine,
        }),
        _ => parse_constraint(labels, rest, raw, line).map(Some),
    }
}

fn parse_constraint(
    labels: Vec<String>,
    tokens: &[Token<'_>],
    raw: &str,
    line: usize,
) -> Result<Item, ParseError> {
    let op_pos = tokens
        .iter()
        .position(|t| matches!(t, Token::GreaterOrEqual | Token::LessOrEqual | Token::Equal))
        .ok_or(ParseError {
            line,
            kind: ParseErrorKind::MissingOperator,
        })?;

    let op = match &tokens[op_pos] {
        Token::GreaterOrEqual => Op::GreaterOrEqual,
        Token::LessOrEqual => Op::LessOrEqual,
        Token::Equal => {
            return Err(ParseError {
                line,
                kind: ParseErrorKind::EqualityNotSupported,
            });
        }
        _ => unreachable!("op_pos was filtered above"),
    };

    let lhs_tokens = &tokens[..op_pos];
    let rhs_tokens = &tokens[op_pos + 1..];

    // A reification shorthand puts its literals and arrow in front of
    // the constraint proper, so split those off before reading terms.
    let (reification, term_tokens) = split_reification(lhs_tokens, line)?;

    let terms = parse_terms(term_tokens, line)?;
    let (rhs, after) = read_rhs(rhs_tokens, line)?;
    require_terminator(after, line)?;

    let constraint = Constraint {
        labels,
        reification,
        terms,
        op,
        rhs,
        line,
        raw: raw.to_owned(),
    };
    check_label_count(&constraint, line)?;
    Ok(Item::Constraint(constraint))
}

/// Split a reification shorthand off the front of a constraint's
/// left-hand side. Returns the shorthand (if any) and the tokens of the
/// constraint proper.
fn split_reification<'a, 'b>(
    lhs: &'a [Token<'b>],
    line: usize,
) -> Result<(Option<Reification>, &'a [Token<'b>]), ParseError> {
    let Some(arrow_pos) = lhs.iter().position(|t| {
        matches!(
            t,
            Token::RightImplication | Token::LeftImplication | Token::Equivalence
        )
    }) else {
        return Ok((None, lhs));
    };

    let arrow = &lhs[arrow_pos];
    let name = arrow_name(arrow);
    let literals = reification_literals(&lhs[..arrow_pos], name, line)?;

    let reification = match arrow {
        Token::RightImplication => Reification::LiteralsImplyConstraint(literals),
        // A left implication and an equivalence reify a single literal.
        Token::LeftImplication => {
            Reification::ConstraintImpliesLiteral(single_literal(literals, name, line)?)
        }
        Token::Equivalence => Reification::Equivalence(single_literal(literals, name, line)?),
        _ => unreachable!("arrow_pos was filtered above"),
    };
    Ok((Some(reification), &lhs[arrow_pos + 1..]))
}

fn arrow_name(token: &Token<'_>) -> &'static str {
    match token {
        Token::RightImplication => "==>",
        Token::LeftImplication => "<==",
        Token::Equivalence => "<==>",
        _ => unreachable!("not an arrow"),
    }
}

/// Read the literals in front of a reification arrow. There has to be
/// at least one, and nothing but literals.
fn reification_literals(
    tokens: &[Token<'_>],
    arrow: &'static str,
    line: usize,
) -> Result<Vec<Literal>, ParseError> {
    if tokens.is_empty() {
        return Err(ParseError {
            line,
            kind: ParseErrorKind::EmptyReification(arrow),
        });
    }
    tokens
        .iter()
        .map(|tok| match tok {
            Token::PositiveLiteral(name) => Ok(Literal {
                variable: (*name).to_owned(),
                negated: false,
            }),
            Token::NegatedLiteral(name) => Ok(Literal {
                variable: (*name).to_owned(),
                negated: true,
            }),
            other => Err(ParseError {
                line,
                kind: ParseErrorKind::NonLiteralInReification {
                    arrow,
                    token: format!("{other:?}"),
                },
            }),
        })
        .collect()
}

fn single_literal(
    literals: Vec<Literal>,
    arrow: &'static str,
    line: usize,
) -> Result<Literal, ParseError> {
    match <[Literal; 1]>::try_from(literals) {
        Ok([literal]) => Ok(literal),
        Err(literals) => Err(ParseError {
            line,
            kind: ParseErrorKind::MultipleLiteralsInReification {
                arrow,
                count: literals.len(),
            },
        }),
    }
}

/// A line carries either no labels at all or exactly one for each
/// constraint it stands for, since the i-th label names the i-th
/// constraint. Any other number is an error, as it is in VeriPB.
fn check_label_count(constraint: &Constraint, line: usize) -> Result<(), ParseError> {
    let expected = constraint.constraint_count();
    let found = constraint.labels.len();
    if found == 0 || found == expected {
        Ok(())
    } else {
        Err(ParseError {
            line,
            kind: ParseErrorKind::LabelCount { expected, found },
        })
    }
}

fn parse_objective(
    tokens: &[Token<'_>],
    raw: &str,
    line: usize,
    maximise: bool,
) -> Result<Item, ParseError> {
    let body = strip_trailing_terminator(tokens, line)?;
    let terms = parse_terms(body, line)?;
    Ok(Item::Objective(Objective {
        terms,
        maximise,
        line,
        raw: raw.to_owned(),
    }))
}

fn parse_preserved(tokens: &[Token<'_>], raw: &str, line: usize) -> Result<Item, ParseError> {
    let body = strip_trailing_terminator(tokens, line)?;
    let mut literals = Vec::new();
    for tok in body {
        match tok {
            Token::PositiveLiteral(name) => literals.push(Literal {
                variable: (*name).to_owned(),
                negated: false,
            }),
            Token::NegatedLiteral(name) => literals.push(Literal {
                variable: (*name).to_owned(),
                negated: true,
            }),
            other => {
                return Err(ParseError {
                    line,
                    kind: ParseErrorKind::UnexpectedTokenInPreserved(format!("{other:?}")),
                });
            }
        }
    }
    Ok(Item::Preserved(Preserved {
        literals,
        line,
        raw: raw.to_owned(),
    }))
}

fn parse_terms(tokens: &[Token<'_>], line: usize) -> Result<Vec<Term>, ParseError> {
    let mut terms = Vec::new();
    let mut current_coef: Option<i64> = None;
    for tok in tokens {
        match tok {
            Token::Coefficient(c) => {
                if let Some(prev) = current_coef.take() {
                    terms.push(Term::Constant(prev));
                }
                current_coef = Some(*c);
            }
            Token::PositiveLiteral(name) => {
                let c = take_coefficient(&mut current_coef, line, name)?;
                terms.push(Term::Linear {
                    coefficient: c,
                    literal: Literal {
                        variable: (*name).to_owned(),
                        negated: false,
                    },
                });
            }
            Token::NegatedLiteral(name) => {
                let c = take_coefficient(&mut current_coef, line, name)?;
                terms.push(Term::Linear {
                    coefficient: c,
                    literal: Literal {
                        variable: (*name).to_owned(),
                        negated: true,
                    },
                });
            }
            Token::BadCoefficient(raw) => {
                return Err(ParseError {
                    line,
                    kind: ParseErrorKind::CoefficientOutOfRange((*raw).to_owned()),
                });
            }
            other => {
                return Err(ParseError {
                    line,
                    kind: ParseErrorKind::UnexpectedToken(format!("{other:?}")),
                });
            }
        }
    }
    if let Some(c) = current_coef {
        terms.push(Term::Constant(c));
    }
    Ok(terms)
}

fn take_coefficient(
    current: &mut Option<i64>,
    line: usize,
    name_for_error: &str,
) -> Result<i64, ParseError> {
    current.take().ok_or_else(|| ParseError {
        line,
        kind: ParseErrorKind::LiteralWithoutCoefficient(name_for_error.to_owned()),
    })
}

fn read_rhs<'a>(
    tokens: &'a [Token<'a>],
    line: usize,
) -> Result<(i64, &'a [Token<'a>]), ParseError> {
    match tokens.first() {
        Some(Token::Coefficient(c)) => Ok((*c, &tokens[1..])),
        Some(Token::BadCoefficient(raw)) => Err(ParseError {
            line,
            kind: ParseErrorKind::CoefficientOutOfRange((*raw).to_owned()),
        }),
        Some(other) => Err(ParseError {
            line,
            kind: ParseErrorKind::ExpectedRhs(format!("{other:?}")),
        }),
        None => Err(ParseError {
            line,
            kind: ParseErrorKind::MissingRhs,
        }),
    }
}

fn require_terminator(tokens: &[Token<'_>], line: usize) -> Result<(), ParseError> {
    match tokens {
        [Token::Semicolon] => Ok(()),
        [] => Err(ParseError {
            line,
            kind: ParseErrorKind::MissingTerminator,
        }),
        _ => Err(ParseError {
            line,
            kind: ParseErrorKind::UnexpectedTrailingTokens(format!("{tokens:?}")),
        }),
    }
}

fn strip_trailing_terminator<'a, 'b>(
    tokens: &'a [Token<'b>],
    line: usize,
) -> Result<&'a [Token<'b>], ParseError> {
    match tokens.last() {
        Some(Token::Semicolon) => Ok(&tokens[..tokens.len() - 1]),
        _ => Err(ParseError {
            line,
            kind: ParseErrorKind::MissingTerminator,
        }),
    }
}

/// A parse error, carrying the source line and a kind.
#[derive(Debug, Clone, thiserror::Error)]
#[error("line {line}: {kind}")]
pub struct ParseError {
    pub line: usize,
    pub kind: ParseErrorKind,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseErrorKind {
    #[error("missing operator (>= or <=)")]
    MissingOperator,
    #[error("missing right-hand side")]
    MissingRhs,
    #[error("expected right-hand side integer, found {0}")]
    ExpectedRhs(String),
    #[error("missing `;` terminator")]
    MissingTerminator,
    #[error("equality constraints (`=`) are not supported")]
    EqualityNotSupported,
    #[error("literal `{0}` has no preceding coefficient")]
    LiteralWithoutCoefficient(String),
    #[error("coefficient `{0}` does not fit in i64")]
    CoefficientOutOfRange(String),
    #[error("unexpected token {0}")]
    UnexpectedToken(String),
    #[error("unexpected trailing tokens {0}")]
    UnexpectedTrailingTokens(String),
    #[error("unexpected token in preserved line: {0}")]
    UnexpectedTokenInPreserved(String),
    #[error("`{0}` needs at least one literal in front of it")]
    EmptyReification(&'static str),
    #[error("expected a literal in front of `{arrow}`, found {token}")]
    NonLiteralInReification { arrow: &'static str, token: String },
    #[error("`{arrow}` reifies a single literal, but found {count}")]
    MultipleLiteralsInReification { arrow: &'static str, count: usize },
    #[error(
        "this line stands for {expected} constraints, so it takes either no labels or {expected}, but found {found}"
    )]
    LabelCount { expected: usize, found: usize },
    #[error("labels can only be attached to constraints")]
    LabelsOnNonConstraintLine,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(input: &str) -> Item {
        let f = parse(input).expect("parse should succeed");
        assert_eq!(f.items.len(), 1, "expected exactly one item");
        f.items.into_iter().next().unwrap()
    }

    #[test]
    fn parses_simple_ge_constraint() {
        let Item::Constraint(c) = parse_one("1 x1 1 x2 >= 2 ;\n") else {
            panic!()
        };
        assert_eq!(c.op, Op::GreaterOrEqual);
        assert_eq!(c.rhs, 2);
        assert!(c.labels.is_empty());
        assert_eq!(c.terms.len(), 2);
    }

    #[test]
    fn parses_le_with_negative_rhs() {
        let Item::Constraint(c) = parse_one("-1 x1 -2 x2 <= -5 ;\n") else {
            panic!()
        };
        assert_eq!(c.op, Op::LessOrEqual);
        assert_eq!(c.rhs, -5);
    }

    #[test]
    fn parses_attached_semicolon() {
        let Item::Constraint(c) = parse_one("1 x1 >= 1;\n") else {
            panic!()
        };
        assert_eq!(c.rhs, 1);
    }

    #[test]
    fn parses_labelled_constraint() {
        let Item::Constraint(c) = parse_one("@cardinality 1 x1 1 x2 1 x3 >= 2 ;\n") else {
            panic!()
        };
        assert_eq!(c.labels, ["cardinality"]);
        assert_eq!(c.terms.len(), 3);
    }

    #[test]
    fn parses_negated_literals_and_bracket_names() {
        let Item::Constraint(c) = parse_one("@c[_1] 16 ~x[money_a_d][0_1] -1 i[a][b0] >= 1 ;\n")
        else {
            panic!()
        };
        assert_eq!(c.labels, ["c[_1]"]);
        let Term::Linear {
            coefficient,
            literal,
        } = &c.terms[0]
        else {
            panic!()
        };
        assert_eq!(*coefficient, 16);
        assert_eq!(literal.variable, "x[money_a_d][0_1]");
        assert!(literal.negated);
    }

    #[test]
    fn parses_right_implication() {
        let Item::Constraint(c) = parse_one("x1 ~x2 ==> 1 x3 1 x4 >= 1 ;\n") else {
            panic!()
        };
        let Some(Reification::LiteralsImplyConstraint(lits)) = &c.reification else {
            panic!("expected a right implication, got {:?}", c.reification)
        };
        assert_eq!(lits.len(), 2);
        assert_eq!(lits[0].variable, "x1");
        assert!(!lits[0].negated);
        assert_eq!(lits[1].variable, "x2");
        assert!(lits[1].negated);
        // The constraint proper is everything after the arrow.
        assert_eq!(c.terms.len(), 2);
        assert_eq!(c.rhs, 1);
        assert_eq!(c.constraint_count(), 1);
    }

    #[test]
    fn parses_left_implication() {
        let Item::Constraint(c) = parse_one("x1 <== 1 x3 1 x4 >= 1 ;\n") else {
            panic!()
        };
        let Some(Reification::ConstraintImpliesLiteral(lit)) = &c.reification else {
            panic!("expected a left implication, got {:?}", c.reification)
        };
        assert_eq!(lit.variable, "x1");
        assert_eq!(c.constraint_count(), 1);
    }

    #[test]
    fn parses_equivalence_with_two_labels() {
        let Item::Constraint(c) = parse_one("@right @left z1 <==> 1 x1 1 x2 >= 1 ;\n") else {
            panic!()
        };
        assert_eq!(c.labels, ["right", "left"]);
        let Some(Reification::Equivalence(lit)) = &c.reification else {
            panic!("expected an equivalence, got {:?}", c.reification)
        };
        assert_eq!(lit.variable, "z1");
        assert_eq!(c.constraint_count(), 2);
    }

    #[test]
    fn equivalence_may_carry_no_labels_at_all() {
        let Item::Constraint(c) = parse_one("z1 <==> 1 x1 >= 1 ;\n") else {
            panic!()
        };
        assert!(c.labels.is_empty());
    }

    #[test]
    fn reification_works_with_a_le_operator() {
        let Item::Constraint(c) = parse_one("z1 <== 1 x1 <= 3 ;\n") else {
            panic!()
        };
        assert_eq!(c.op, Op::LessOrEqual);
        assert!(matches!(
            c.reification,
            Some(Reification::ConstraintImpliesLiteral(_))
        ));
    }

    #[test]
    fn left_implication_rejects_several_literals() {
        // `<==` and `<==>` reify a single literal; only `==>` takes a
        // conjunction.
        let err = parse("x1 x2 <== 1 x3 >= 1 ;\n").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::MultipleLiteralsInReification {
                arrow: "<==",
                count: 2
            }
        ));
    }

    #[test]
    fn equivalence_rejects_several_literals() {
        let err = parse("x1 x2 <==> 1 x3 >= 1 ;\n").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::MultipleLiteralsInReification { arrow: "<==>", .. }
        ));
    }

    #[test]
    fn reification_needs_at_least_one_literal() {
        let err = parse("==> 1 x1 >= 1 ;\n").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::EmptyReification("==>")));
    }

    #[test]
    fn only_literals_may_precede_an_arrow() {
        let err = parse("1 x1 ==> 1 x2 >= 1 ;\n").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::NonLiteralInReification { arrow: "==>", .. }
        ));
    }

    #[test]
    fn a_line_takes_no_labels_or_one_per_constraint() {
        // An equivalence stands for two constraints, so one label is
        // ambiguous: which of the two would it name?
        let err = parse("@only z1 <==> 1 x1 >= 1 ;\n").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::LabelCount {
                expected: 2,
                found: 1
            }
        ));

        // And an ordinary constraint stands for one, so two is an error
        // the other way round.
        let err = parse("@one @two 1 x1 >= 1 ;\n").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::LabelCount {
                expected: 1,
                found: 2
            }
        ));
    }

    #[test]
    fn labels_are_rejected_on_non_constraint_lines() {
        for input in ["@l min: 1 x1 ;\n", "@l preserved: x1 ;\n"] {
            let err = parse(input).unwrap_err();
            assert!(
                matches!(err.kind, ParseErrorKind::LabelsOnNonConstraintLine),
                "got {:?} for {input}",
                err.kind
            );
        }
    }

    #[test]
    fn parses_min_objective() {
        let Item::Objective(o) = parse_one("min: 1 ~x1 1 ~x2 1 ~x3 ;\n") else {
            panic!()
        };
        assert_eq!(o.terms.len(), 3);
    }

    #[test]
    fn parses_max_objective() {
        let Item::Objective(o) = parse_one("max: 1 x1 2 x2 ;\n") else {
            panic!()
        };
        assert!(o.maximise);
        assert_eq!(o.terms.len(), 2);
    }

    #[test]
    fn min_objective_is_not_marked_as_maximising() {
        let Item::Objective(o) = parse_one("min: 1 x1 ;\n") else {
            panic!()
        };
        assert!(!o.maximise);
    }

    #[test]
    fn parses_preserved() {
        let Item::Preserved(p) = parse_one("preserved: x1 x3;\n") else {
            panic!()
        };
        assert_eq!(p.literals.len(), 2);
        assert_eq!(p.literals[0].variable, "x1");
        assert!(!p.literals[0].negated);
    }

    #[test]
    fn ignores_comments_and_blanks() {
        let input = "* header here\n* #variable= 3 #constraint= 1\n\n1 x1 >= 1 ;\n";
        let f = parse(input).unwrap();
        assert_eq!(f.items.len(), 1);
    }

    #[test]
    fn comments_interleaved_with_constraints() {
        let input = "1 x1 >= 1 ;\n* mid-file comment\n1 x2 >= 1 ;\n";
        let f = parse(input).unwrap();
        assert_eq!(f.items.len(), 2);
    }

    #[test]
    fn equality_is_rejected() {
        let err = parse("1 x1 = 1 ;\n").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::EqualityNotSupported));
        assert_eq!(err.line, 1);
    }

    #[test]
    fn missing_operator_is_rejected() {
        let err = parse("1 x1 1 x2 ;\n").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::MissingOperator));
    }

    #[test]
    fn missing_terminator_is_rejected() {
        let err = parse("1 x1 >= 1\n").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::MissingTerminator));
    }

    #[test]
    fn literal_without_coefficient_is_rejected() {
        let err = parse("x1 >= 1 ;\n").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::LiteralWithoutCoefficient(_)
        ));
    }

    #[test]
    fn line_numbers_track_source() {
        let input = "* comment\n\n1 x1 >= 1 ;\n1 x2 >= 1 ;\n";
        let f = parse(input).unwrap();
        let Item::Constraint(a) = &f.items[0] else {
            panic!()
        };
        let Item::Constraint(b) = &f.items[1] else {
            panic!()
        };
        assert_eq!(a.line, 3);
        assert_eq!(b.line, 4);
    }

    #[test]
    fn explicit_plus_sign_on_later_terms() {
        // From add_preserved_var.opb: `1 x1 +1 x2 >= 1 ;`
        let Item::Constraint(c) = parse_one("1 x1 +1 x2 >= 1 ;\n") else {
            panic!()
        };
        assert_eq!(c.terms.len(), 2);
        for term in &c.terms {
            let Term::Linear { coefficient, .. } = term else {
                panic!()
            };
            assert_eq!(*coefficient, 1);
        }
    }

    #[test]
    fn bare_constant_becomes_constant_term() {
        // A coefficient with no following literal is an LHS constant.
        let Item::Constraint(c) = parse_one("1 x1 -2 >= 0 ;\n") else {
            panic!()
        };
        assert_eq!(c.terms.len(), 2);
        assert!(matches!(c.terms[1], Term::Constant(-2)));
    }
}
