//! Whitespace-based tokeniser for a single OPB line.
//!
//! OPB is line-oriented and almost entirely whitespace-separated; the only
//! exception we have seen in real files is the `;` terminator, which may be
//! either a standalone token or attached to the preceding token (`>= 1;`
//! vs `>= 1 ;`). We handle that by splitting trailing `;` off any raw
//! token before classification.
//!
//! Every other operator — `>=`, `<=`, and the reification arrows `==>`,
//! `<==` and `<==>` — must be whitespace-separated. VeriPB's own OPB
//! lexer does not require that (`z1<==>1 x1 >= 1` is legal there), but
//! every OPB file we have seen writes operators with surrounding
//! whitespace, and accepting attached arrows while still rejecting an
//! attached `>=` would be a confusing half-measure. See
//! `dev_docs/0002-opb-format.md`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    /// A numeric coefficient, with sign attached.
    Coefficient(i64),
    /// A positive literal (variable name).
    PositiveLiteral(&'a str),
    /// A negated literal (`~x`), with the `~` stripped.
    NegatedLiteral(&'a str),
    /// A constraint label (`@name`), with the `@` stripped.
    Label(&'a str),
    GreaterOrEqual,
    LessOrEqual,
    Equal,
    /// The `==>` arrow of a right implication.
    RightImplication,
    /// The `<==` arrow of a left implication.
    LeftImplication,
    /// The `<==>` arrow of an equivalence.
    Equivalence,
    Semicolon,
    /// The `min:` keyword introducing a minimisation objective line.
    Min,
    /// The `max:` keyword introducing a maximisation objective line.
    Max,
    /// The `preserved:` keyword introducing a preserved-variables line.
    Preserved,
    /// A token that looked like a coefficient but did not fit in `i64`.
    BadCoefficient(&'a str),
}

pub(crate) fn tokenize_line(line: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    for raw in line.split_whitespace() {
        let (body, trailing_semi) = match raw.strip_suffix(';') {
            Some(stripped) => (stripped, true),
            None => (raw, false),
        };
        if !body.is_empty() {
            tokens.push(classify(body));
        }
        if trailing_semi {
            tokens.push(Token::Semicolon);
        }
    }
    tokens
}

fn classify(token: &str) -> Token<'_> {
    match token {
        ">=" => Token::GreaterOrEqual,
        "<=" => Token::LessOrEqual,
        "=" => Token::Equal,
        "==>" => Token::RightImplication,
        "<==" => Token::LeftImplication,
        "<==>" => Token::Equivalence,
        ";" => Token::Semicolon,
        "min:" => Token::Min,
        "max:" => Token::Max,
        "preserved:" => Token::Preserved,
        _ => {
            if let Some(name) = token.strip_prefix('@') {
                Token::Label(name)
            } else if let Some(name) = token.strip_prefix('~') {
                Token::NegatedLiteral(name)
            } else if looks_like_coefficient(token) {
                match token.parse::<i64>() {
                    Ok(n) => Token::Coefficient(n),
                    Err(_) => Token::BadCoefficient(token),
                }
            } else {
                Token::PositiveLiteral(token)
            }
        }
    }
}

fn looks_like_coefficient(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let rest = match bytes[0] {
        b'+' | b'-' => &bytes[1..],
        _ => bytes,
    };
    !rest.is_empty() && rest.iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_basic_tokens() {
        assert_eq!(classify(">="), Token::GreaterOrEqual);
        assert_eq!(classify("<="), Token::LessOrEqual);
        assert_eq!(classify("="), Token::Equal);
        assert_eq!(classify(";"), Token::Semicolon);
        assert_eq!(classify("min:"), Token::Min);
        assert_eq!(classify("max:"), Token::Max);
        assert_eq!(classify("preserved:"), Token::Preserved);
    }

    #[test]
    fn classifies_reification_arrows() {
        assert_eq!(classify("==>"), Token::RightImplication);
        assert_eq!(classify("<=="), Token::LeftImplication);
        assert_eq!(classify("<==>"), Token::Equivalence);
    }

    #[test]
    fn tokenizes_a_right_implication_line() {
        assert_eq!(
            tokenize_line("x1 ~x2 ==> 1 x3 >= 1 ;"),
            vec![
                Token::PositiveLiteral("x1"),
                Token::NegatedLiteral("x2"),
                Token::RightImplication,
                Token::Coefficient(1),
                Token::PositiveLiteral("x3"),
                Token::GreaterOrEqual,
                Token::Coefficient(1),
                Token::Semicolon,
            ],
        );
    }

    #[test]
    fn classifies_coefficients() {
        assert_eq!(classify("1"), Token::Coefficient(1));
        assert_eq!(classify("+1"), Token::Coefficient(1));
        assert_eq!(classify("-1"), Token::Coefficient(-1));
        assert_eq!(classify("0"), Token::Coefficient(0));
        assert_eq!(classify("-123"), Token::Coefficient(-123));
    }

    #[test]
    fn classifies_literals() {
        assert_eq!(classify("x1"), Token::PositiveLiteral("x1"));
        assert_eq!(classify("~x1"), Token::NegatedLiteral("x1"));
        assert_eq!(classify("y_x1_5"), Token::PositiveLiteral("y_x1_5"));
        assert_eq!(classify("i[a][b0]"), Token::PositiveLiteral("i[a][b0]"));
        assert_eq!(
            classify("~f[0][notequals]"),
            Token::NegatedLiteral("f[0][notequals]"),
        );
    }

    #[test]
    fn classifies_labels() {
        assert_eq!(classify("@card"), Token::Label("card"));
        assert_eq!(classify("@i[a][lb]"), Token::Label("i[a][lb]"));
        assert_eq!(
            classify("@c[money_a_d][[1]gt[2]]"),
            Token::Label("c[money_a_d][[1]gt[2]]"),
        );
    }

    #[test]
    fn handles_attached_semicolon() {
        assert_eq!(
            tokenize_line("1 x1 >= 1;"),
            vec![
                Token::Coefficient(1),
                Token::PositiveLiteral("x1"),
                Token::GreaterOrEqual,
                Token::Coefficient(1),
                Token::Semicolon,
            ],
        );
    }

    #[test]
    fn handles_detached_semicolon() {
        assert_eq!(
            tokenize_line("1 x1 >= 1 ;"),
            vec![
                Token::Coefficient(1),
                Token::PositiveLiteral("x1"),
                Token::GreaterOrEqual,
                Token::Coefficient(1),
                Token::Semicolon,
            ],
        );
    }

    #[test]
    fn handles_negated_literal_with_attached_semicolon() {
        // From a real file: `... 16 ~x[money_a_d][0_1] >= 1 ;`
        assert_eq!(
            tokenize_line("16 ~x[money_a_d][0_1] >= 1 ;"),
            vec![
                Token::Coefficient(16),
                Token::NegatedLiteral("x[money_a_d][0_1]"),
                Token::GreaterOrEqual,
                Token::Coefficient(1),
                Token::Semicolon,
            ],
        );
    }

    #[test]
    fn coefficient_overflow_is_bad_coefficient() {
        let big = "999999999999999999999999";
        assert_eq!(classify(big), Token::BadCoefficient(big));
    }

    #[test]
    fn ignores_extra_whitespace() {
        assert_eq!(
            tokenize_line("   1   x1   >=  1 ;   "),
            vec![
                Token::Coefficient(1),
                Token::PositiveLiteral("x1"),
                Token::GreaterOrEqual,
                Token::Coefficient(1),
                Token::Semicolon,
            ],
        );
    }
}
