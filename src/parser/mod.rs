//! OPB parser. Line-oriented, building an AST that preserves source
//! information for the reporter. Comments and blank lines are dropped.

mod ast;
mod lexer;

pub use ast::{Constraint, Item, Literal, Objective, Op, OpbFile, Preserved, Term};

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
    match &tokens[0] {
        Token::Min => parse_objective(&tokens[1..], raw, line).map(Some),
        Token::Preserved => parse_preserved(&tokens[1..], raw, line).map(Some),
        Token::Label(name) => {
            let name = (*name).to_owned();
            parse_constraint(Some(name), &tokens[1..], raw, line).map(Some)
        }
        _ => parse_constraint(None, &tokens, raw, line).map(Some),
    }
}

fn parse_constraint(
    label: Option<String>,
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

    let terms = parse_terms(lhs_tokens, line)?;
    let (rhs, after) = read_rhs(rhs_tokens, line)?;
    require_terminator(after, line)?;

    Ok(Item::Constraint(Constraint {
        label,
        terms,
        op,
        rhs,
        line,
        raw: raw.to_owned(),
    }))
}

fn parse_objective(tokens: &[Token<'_>], raw: &str, line: usize) -> Result<Item, ParseError> {
    let body = strip_trailing_terminator(tokens, line)?;
    let terms = parse_terms(body, line)?;
    Ok(Item::Objective(Objective {
        terms,
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
        assert_eq!(c.label, None);
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
        assert_eq!(c.label.as_deref(), Some("cardinality"));
        assert_eq!(c.terms.len(), 3);
    }

    #[test]
    fn parses_negated_literals_and_bracket_names() {
        let Item::Constraint(c) = parse_one("@c[_1] 16 ~x[money_a_d][0_1] -1 i[a][b0] >= 1 ;\n")
        else {
            panic!()
        };
        assert_eq!(c.label.as_deref(), Some("c[_1]"));
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
    fn parses_min_objective() {
        let Item::Objective(o) = parse_one("min: 1 ~x1 1 ~x2 1 ~x3 ;\n") else {
            panic!()
        };
        assert_eq!(o.terms.len(), 3);
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
