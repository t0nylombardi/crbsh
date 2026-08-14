use crate::tokens::{Token, TokenizeError, tokenize};

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Tokenize(TokenizeError),
    EmptyCommand,
    UnexpectedToken(Token),
}

impl From<TokenizeError> for ParseError {
    fn from(error: TokenizeError) -> Self {
        Self::Tokenize(error)
    }
}

pub fn parse(input: &str) -> Result<ParsedCommand, ParseError> {
    let tokens = tokenize(input)?;

    let mut tokens = tokens.into_iter();

    let name = match tokens.next() {
        Some(Token::Word(name)) => name,
        Some(token) => return Err(ParseError::UnexpectedToken(token)),
        None => return Err(ParseError::EmptyCommand),
    };

    let mut args = Vec::new();

    for token in tokens {
        match token {
            Token::Word(value) => args.push(value),

            token => {
                return Err(ParseError::UnexpectedToken(token));
            }
        }
    }

    Ok(ParsedCommand { name, args })
}

#[test]
fn rejects_unterminated_double_quote() {
    let result = parse(r#"print "hello"#);

    assert_eq!(
        result,
        Err(ParseError::Tokenize(TokenizeError::UnterminatedDoubleQuote))
    );
}

#[test]
fn rejects_unterminated_single_quote() {
    let result = parse("print 'hello");

    assert_eq!(
        result,
        Err(ParseError::Tokenize(TokenizeError::UnterminatedSingleQuote))
    );
}

#[test]
fn rejects_trailing_escape() {
    let result = parse("print hello\\");

    assert_eq!(
        result,
        Err(ParseError::Tokenize(TokenizeError::TrailingEscape))
    );
}
