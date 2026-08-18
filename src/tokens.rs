#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    StringLiteral(String),
    IntLiteral(i64),
    BoolLiteral(bool),
    Assign,
    Equal,
    NotEqual,
    FatArrow,
    Arrow,
    Colon,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    LeftParen,
    RightParen,
    LessEqual,
    GreaterEqual,
    LeftBrace,
    RightBrace,
    Wildcard,
    Pipe,
    RedirectOut,
    RedirectAppend,
    RedirectIn,
    Background,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenizeError {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    TrailingEscape,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, TokenizeError> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    let mut chars = input.chars().peekable();

    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut quoted = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quotes => {
                escaped = true;
            }

            '\'' if !in_double_quotes => {
                quoted = true;
                in_single_quotes = !in_single_quotes;
            }

            '"' if !in_single_quotes => {
                quoted = true;
                in_double_quotes = !in_double_quotes;
            }

            '|' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::Pipe);
            }

            '>' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);

                if matches!(chars.peek(), Some('>')) {
                    chars.next();
                    tokens.push(Token::RedirectAppend);
                } else if matches!(chars.peek(), Some('=')) {
                    chars.next();
                    tokens.push(Token::GreaterEqual);
                } else {
                    tokens.push(Token::RedirectOut);
                }
            }

            '<' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);

                if matches!(chars.peek(), Some('=')) {
                    chars.next();
                    tokens.push(Token::LessEqual);
                } else {
                    tokens.push(Token::RedirectIn);
                }
            }

            '&' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::Background);
            }

            '=' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);

                if matches!(chars.peek(), Some('>')) {
                    chars.next();
                    tokens.push(Token::FatArrow);
                } else if matches!(chars.peek(), Some('=')) {
                    chars.next();
                    tokens.push(Token::Equal);
                } else {
                    tokens.push(Token::Assign);
                }
            }

            '{' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::LeftBrace);
            }

            '}' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::RightBrace);
            }

            '!' if !in_single_quotes && !in_double_quotes && matches!(chars.peek(), Some('=')) => {
                push_word(&mut tokens, &mut current, &mut quoted);
                chars.next();
                tokens.push(Token::NotEqual);
            }

            ':' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::Colon);
            }

            ',' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::Comma);
            }

            '(' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::LeftParen);
            }

            ')' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
                tokens.push(Token::RightParen);
            }

            '+' if !in_single_quotes
                && !in_double_quotes
                && current.is_empty()
                && is_separated_operator(chars.peek()) =>
            {
                tokens.push(Token::Plus);
            }

            '-' if !in_single_quotes
                && !in_double_quotes
                && current.is_empty()
                && matches!(chars.peek(), Some('>')) =>
            {
                chars.next();
                tokens.push(Token::Arrow);
            }

            '-' if !in_single_quotes
                && !in_double_quotes
                && current.is_empty()
                && is_separated_operator(chars.peek()) =>
            {
                tokens.push(Token::Minus);
            }

            '*' if !in_single_quotes
                && !in_double_quotes
                && current.is_empty()
                && is_separated_operator(chars.peek()) =>
            {
                tokens.push(Token::Star);
            }

            '/' if !in_single_quotes
                && !in_double_quotes
                && current.is_empty()
                && is_separated_operator(chars.peek()) =>
            {
                tokens.push(Token::Slash);
            }

            ch if ch.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current, &mut quoted);
            }

            _ => {
                current.push(ch);
            }
        }
    }

    if escaped {
        return Err(TokenizeError::TrailingEscape);
    }

    if in_single_quotes {
        return Err(TokenizeError::UnterminatedSingleQuote);
    }

    if in_double_quotes {
        return Err(TokenizeError::UnterminatedDoubleQuote);
    }

    push_word(&mut tokens, &mut current, &mut quoted);

    Ok(tokens)
}

fn push_word(tokens: &mut Vec<Token>, current: &mut String, quoted: &mut bool) {
    if !current.is_empty() {
        let value = std::mem::take(current);

        if *quoted {
            tokens.push(Token::StringLiteral(value));
        } else {
            tokens.push(classify_word(value));
        }

        *quoted = false;
    }
}

fn classify_word(value: String) -> Token {
    match value.as_str() {
        "true" => Token::BoolLiteral(true),
        "false" => Token::BoolLiteral(false),
        "_" => Token::Wildcard,
        _ => value
            .parse::<i64>()
            .map(Token::IntLiteral)
            .unwrap_or(Token::Word(value)),
    }
}

fn is_separated_operator(next: Option<&char>) -> bool {
    next.is_none_or(|ch| ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_simple_command() {
        let tokens = tokenize("print hello").unwrap();

        assert_eq!(
            tokens,
            vec![Token::Word("print".into()), Token::Word("hello".into())]
        );
    }

    #[test]
    fn tokenizes_pipe() {
        let tokens = tokenize("ls | grep crab").unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("ls".into()),
                Token::Pipe,
                Token::Word("grep".into()),
                Token::Word("crab".into()),
            ]
        );
    }

    #[test]
    fn tokenizes_redirect_out() {
        let tokens = tokenize("print hello > out.txt").unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("print".into()),
                Token::Word("hello".into()),
                Token::RedirectOut,
                Token::Word("out.txt".into()),
            ]
        );
    }

    #[test]
    fn tokenizes_redirect_append() {
        let tokens = tokenize("print hello >> out.txt").unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("print".into()),
                Token::Word("hello".into()),
                Token::RedirectAppend,
                Token::Word("out.txt".into()),
            ]
        );
    }

    #[test]
    fn tokenizes_redirect_in() {
        let tokens = tokenize("cat < input.txt").unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("cat".into()),
                Token::RedirectIn,
                Token::Word("input.txt".into()),
            ]
        );
    }

    #[test]
    fn tokenizes_background() {
        let tokens = tokenize("sleep 10 &").unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("sleep".into()),
                Token::IntLiteral(10),
                Token::Background,
            ]
        );
    }

    #[test]
    fn operators_inside_quotes_are_words() {
        let tokens = tokenize(r#"print "hello | crab > file""#).unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("print".into()),
                Token::StringLiteral("hello | crab > file".into()),
            ]
        );
    }

    #[test]
    fn escaped_operator_is_word_content() {
        let tokens = tokenize(r"print \|").unwrap();

        assert_eq!(
            tokens,
            vec![Token::Word("print".into()), Token::Word("|".into())]
        );
    }

    #[test]
    fn tokenizes_assignment_and_type_annotation() {
        let tokens = tokenize(r#"let retries: int = 3"#).unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("let".into()),
                Token::Word("retries".into()),
                Token::Colon,
                Token::Word("int".into()),
                Token::Assign,
                Token::IntLiteral(3),
            ]
        );
    }

    #[test]
    fn tokenizes_bool_and_quoted_bool_differently() {
        let tokens = tokenize(r#"print true "true""#).unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("print".into()),
                Token::BoolLiteral(true),
                Token::StringLiteral("true".into()),
            ]
        );
    }

    #[test]
    fn tokenizes_expression_operators() {
        let tokens = tokenize("let ready = retries + 1 <= 5 == true").unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("let".into()),
                Token::Word("ready".into()),
                Token::Assign,
                Token::Word("retries".into()),
                Token::Plus,
                Token::IntLiteral(1),
                Token::LessEqual,
                Token::IntLiteral(5),
                Token::Equal,
                Token::BoolLiteral(true),
            ]
        );
    }

    #[test]
    fn tokenizes_blocks_and_match_arrow() {
        let tokens = tokenize(r#"match status { _ => print "unknown" }"#).unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("match".into()),
                Token::Word("status".into()),
                Token::LeftBrace,
                Token::Wildcard,
                Token::FatArrow,
                Token::Word("print".into()),
                Token::StringLiteral("unknown".into()),
                Token::RightBrace,
            ]
        );
    }

    #[test]
    fn tokenizes_function_signature_and_call() {
        let tokens = tokenize("fn add(a: int, b: int) -> int {").unwrap();

        assert_eq!(
            tokens,
            vec![
                Token::Word("fn".into()),
                Token::Word("add".into()),
                Token::LeftParen,
                Token::Word("a".into()),
                Token::Colon,
                Token::Word("int".into()),
                Token::Comma,
                Token::Word("b".into()),
                Token::Colon,
                Token::Word("int".into()),
                Token::RightParen,
                Token::Arrow,
                Token::Word("int".into()),
                Token::LeftBrace,
            ]
        );

        assert_eq!(
            tokenize("add(2, 3)").unwrap(),
            vec![
                Token::Word("add".into()),
                Token::LeftParen,
                Token::IntLiteral(2),
                Token::Comma,
                Token::IntLiteral(3),
                Token::RightParen,
            ]
        );
    }

    #[test]
    fn keeps_unix_flags_as_words() {
        let tokens = tokenize("ls -la").unwrap();

        assert_eq!(
            tokens,
            vec![Token::Word("ls".into()), Token::Word("-la".into())]
        );
    }

    #[test]
    fn reports_unterminated_double_quote() {
        assert_eq!(
            tokenize(r#"print "hello"#),
            Err(TokenizeError::UnterminatedDoubleQuote)
        );
    }

    #[test]
    fn reports_trailing_escape() {
        assert_eq!(
            tokenize("print hello\\"),
            Err(TokenizeError::TrailingEscape)
        );
    }
}
