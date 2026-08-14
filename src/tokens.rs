#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
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
                in_single_quotes = !in_single_quotes;
            }

            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
            }

            '|' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current);
                tokens.push(Token::Pipe);
            }

            '>' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current);

                if matches!(chars.peek(), Some('>')) {
                    chars.next();
                    tokens.push(Token::RedirectAppend);
                } else {
                    tokens.push(Token::RedirectOut);
                }
            }

            '<' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current);
                tokens.push(Token::RedirectIn);
            }

            '&' if !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current);
                tokens.push(Token::Background);
            }

            ch if ch.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                push_word(&mut tokens, &mut current);
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

    push_word(&mut tokens, &mut current);

    Ok(tokens)
}

fn push_word(tokens: &mut Vec<Token>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(Token::Word(std::mem::take(current)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_simple_command() {
        let tokens = tokenize("print hello").unwrap();

        assert_eq!(
            tokens,
            vec![Token::Word("print".into()), Token::Word("hello".into()),]
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
                Token::Word("10".into()),
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
                Token::Word("hello | crab > file".into()),
            ]
        );
    }

    #[test]
    fn escaped_operator_is_word_content() {
        let tokens = tokenize(r"print \|").unwrap();

        assert_eq!(
            tokens,
            vec![Token::Word("print".into()), Token::Word("|".into()),]
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
