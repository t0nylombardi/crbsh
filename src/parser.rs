use crate::tokens::{Token, TokenizeError, tokenize};

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<ParsedCommand>,
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

pub fn parse(input: &str) -> Result<Pipeline, ParseError> {
    let tokens = tokenize(input)?;

    if tokens.is_empty() {
        return Err(ParseError::EmptyCommand);
    }

    let mut commands = Vec::new();
    let mut current: Option<ParsedCommand> = None;

    for token in tokens {
        match token {
            Token::Word(value) => match &mut current {
                Some(command) => command.args.push(value),
                None => {
                    current = Some(ParsedCommand {
                        name: value,
                        args: Vec::new(),
                    });
                }
            },

            Token::Pipe => match current.take() {
                Some(command) => commands.push(command),
                None => return Err(ParseError::UnexpectedToken(Token::Pipe)),
            },

            token => {
                return Err(ParseError::UnexpectedToken(token));
            }
        }
    }

    match current {
        Some(command) => commands.push(command),
        None => return Err(ParseError::UnexpectedToken(Token::Pipe)),
    }

    Ok(Pipeline { commands })
}

#[test]
fn parses_simple_command() {
    let result = parse("print hello").unwrap();

    assert_eq!(
        result,
        Pipeline {
            commands: vec![ParsedCommand {
                name: "print".into(),
                args: vec!["hello".into()],
            }],
        }
    );
}

#[test]
fn parses_pipeline() {
    let result = parse("ls -la | grep rs | sort").unwrap();

    assert_eq!(
        result,
        Pipeline {
            commands: vec![
                ParsedCommand {
                    name: "ls".into(),
                    args: vec!["-la".into()],
                },
                ParsedCommand {
                    name: "grep".into(),
                    args: vec!["rs".into()],
                },
                ParsedCommand {
                    name: "sort".into(),
                    args: Vec::new(),
                },
            ],
        }
    );
}

#[test]
fn rejects_leading_pipe() {
    let result = parse("|");

    assert_eq!(result, Err(ParseError::UnexpectedToken(Token::Pipe)));
}

#[test]
fn rejects_trailing_pipe() {
    let result = parse("ls |");

    assert_eq!(result, Err(ParseError::UnexpectedToken(Token::Pipe)));
}

#[test]
fn rejects_adjacent_pipes() {
    let result = parse("ls || grep");

    assert_eq!(result, Err(ParseError::UnexpectedToken(Token::Pipe)));
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
