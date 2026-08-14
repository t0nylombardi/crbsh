#[derive(Debug, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    TrailingEscape,
    EmptyCommand,
}

pub fn parse(input: &str) -> Result<ParsedCommand, ParseError> {
    let tokens = tokenize(input)?;

    let mut tokens = tokens.into_iter();

    let name = tokens.next().ok_or(ParseError::EmptyCommand)?;

    Ok(ParsedCommand {
        name,
        args: tokens.collect(),
    })
}

fn tokenize(input: &str) -> Result<Vec<String>, ParseError> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    let mut chars = input.chars();

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

            ch if ch.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }

            _ => {
                current.push(ch);
            }
        }
    }

    if escaped {
        return Err(ParseError::TrailingEscape);
    }

    if in_single_quotes {
        return Err(ParseError::UnterminatedSingleQuote);
    }

    if in_double_quotes {
        return Err(ParseError::UnterminatedDoubleQuote);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_command() {
        let result = parse("print hello").unwrap();

        assert_eq!(
            result,
            ParsedCommand {
                name: "print".to_string(),
                args: vec!["hello".to_string()],
            }
        );
    }

    #[test]
    fn parses_multiple_arguments() {
        let result = parse("print hello crab").unwrap();

        assert_eq!(result.args, vec!["hello", "crab"]);
    }

    #[test]
    fn parses_double_quoted_argument() {
        let result = parse(r#"print "hello crab""#).unwrap();

        assert_eq!(result.args, vec!["hello crab"]);
    }

    #[test]
    fn parses_single_quoted_argument() {
        let result = parse("print 'hello crab'").unwrap();

        assert_eq!(result.args, vec!["hello crab"]);
    }

    #[test]
    fn parses_escaped_spaces() {
        let result = parse(r"print hello\ crab").unwrap();

        assert_eq!(result.args, vec!["hello crab"]);
    }

    #[test]
    fn preserves_whitespace_inside_quotes() {
        let result = parse(r#"print "hello   crab""#).unwrap();

        assert_eq!(result.args, vec!["hello   crab"]);
    }

    #[test]
    fn rejects_unterminated_double_quote() {
        let result = parse(r#"print "hello"#);

        assert_eq!(result, Err(ParseError::UnterminatedDoubleQuote));
    }

    #[test]
    fn rejects_unterminated_single_quote() {
        let result = parse("print 'hello");

        assert_eq!(result, Err(ParseError::UnterminatedSingleQuote));
    }

    #[test]
    fn rejects_trailing_escape() {
        let result = parse("print hello\\");

        assert_eq!(result, Err(ParseError::TrailingEscape));
    }

    #[test]
    fn rejects_empty_input() {
        let result = parse("   ");

        assert_eq!(result, Err(ParseError::EmptyCommand));
    }
}
