use crate::tokens::{Token, TokenizeError, tokenize};
use crate::value::{TypeName, Value};

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<Expression>,
    pub redirections: Redirections,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression {
    Literal(Value),
    Identifier(String),
    EnvironmentVariable(String),
    Status,
}

impl From<&str> for Expression {
    fn from(value: &str) -> Self {
        word_to_expression(value.into())
    }
}

impl From<Value> for Expression {
    fn from(value: Value) -> Self {
        Self::Literal(value)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Redirections {
    pub stdin: Option<String>,
    pub stdout: Option<OutputRedirection>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OutputRedirection {
    pub target: String,
    pub append: bool,
}

impl Redirections {
    pub fn is_empty(&self) -> bool {
        self.stdin.is_none() && self.stdout.is_none()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<ParsedCommand>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParsedInput {
    Pipeline(Pipeline),
    Let {
        name: String,
        type_annotation: Option<TypeName>,
        value: Expression,
    },
    Assignment {
        name: String,
        value: Expression,
    },
    EnvironmentAssignment {
        name: String,
        value: Expression,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Tokenize(TokenizeError),
    EmptyCommand,
    ExpectedAssignmentOperator,
    InvalidEnvironmentName(String),
    InvalidTypeName(String),
    InvalidVariableName(String),
    MissingAssignmentValue,
    MissingRedirectionTarget,
    ReservedName(String),
    UnsupportedRedirection(Token),
    UnexpectedToken(Token),
}

impl From<TokenizeError> for ParseError {
    fn from(error: TokenizeError) -> Self {
        Self::Tokenize(error)
    }
}

pub fn parse(input: &str) -> Result<ParsedInput, ParseError> {
    let tokens = tokenize(input)?;

    if tokens.is_empty() {
        return Err(ParseError::EmptyCommand);
    }

    match tokens.as_slice() {
        [Token::Word(keyword), rest @ ..] if keyword == "let" => {
            return parse_let(rest);
        }
        [Token::Word(name), Token::Assign, rest @ ..] if name.starts_with("env.") => {
            return parse_environment_assignment(name, rest);
        }
        [Token::Word(name), Token::Assign, rest @ ..] if is_valid_identifier(name) => {
            return parse_assignment(name, rest);
        }
        [Token::Word(name)] if name == "env." => {
            return Err(ParseError::UnexpectedToken(Token::Word(name.clone())));
        }
        _ => {}
    }

    parse_pipeline(tokens).map(ParsedInput::Pipeline)
}

fn parse_let(tokens: &[Token]) -> Result<ParsedInput, ParseError> {
    let Some(Token::Word(name)) = tokens.first() else {
        return Err(ParseError::InvalidVariableName(String::new()));
    };

    if !is_valid_identifier(name) {
        return Err(ParseError::InvalidVariableName(name.into()));
    }

    if is_reserved_name(name) {
        return Err(ParseError::ReservedName(name.into()));
    }

    let (type_annotation, rest) = match &tokens[1..] {
        [Token::Colon, Token::Word(type_name), rest @ ..] => {
            let Some(type_name) = TypeName::parse(type_name) else {
                return Err(ParseError::InvalidTypeName(type_name.clone()));
            };

            (Some(type_name), rest)
        }
        rest => (None, rest),
    };

    let value = parse_assignment_tail(rest)?;

    Ok(ParsedInput::Let {
        name: name.into(),
        type_annotation,
        value,
    })
}

fn parse_assignment(name: &str, rest: &[Token]) -> Result<ParsedInput, ParseError> {
    if is_reserved_name(name) {
        return Err(ParseError::ReservedName(name.into()));
    }

    let value = parse_assignment_value(rest)?;

    Ok(ParsedInput::Assignment {
        name: name.into(),
        value,
    })
}

fn parse_environment_assignment(name: &str, rest: &[Token]) -> Result<ParsedInput, ParseError> {
    let Some(name) = name.strip_prefix("env.") else {
        unreachable!("environment assignments are prefiltered by prefix");
    };

    if !is_valid_environment_name(name) {
        return Err(ParseError::InvalidEnvironmentName(name.into()));
    }

    let value = parse_assignment_value(rest)?;

    Ok(ParsedInput::EnvironmentAssignment {
        name: name.into(),
        value,
    })
}

fn parse_assignment_tail(tokens: &[Token]) -> Result<Expression, ParseError> {
    if matches!(tokens, [Token::Assign]) {
        return Err(ParseError::MissingAssignmentValue);
    }

    let [Token::Assign, value] = tokens else {
        return Err(ParseError::ExpectedAssignmentOperator);
    };

    token_to_expression(value).ok_or_else(|| ParseError::UnexpectedToken(value.clone()))
}

fn parse_assignment_value(tokens: &[Token]) -> Result<Expression, ParseError> {
    let [value] = tokens else {
        if tokens.is_empty() {
            return Err(ParseError::MissingAssignmentValue);
        }

        return Err(ParseError::ExpectedAssignmentOperator);
    };

    token_to_expression(value).ok_or_else(|| ParseError::UnexpectedToken(value.clone()))
}

fn parse_pipeline(tokens: Vec<Token>) -> Result<Pipeline, ParseError> {
    let mut commands = Vec::new();
    let mut current: Option<ParsedCommand> = None;
    let mut pending_redirection = None;

    for token in tokens {
        if let Some(redirection) = pending_redirection.take() {
            let target =
                token_to_redirection_target(&token).ok_or(ParseError::UnexpectedToken(token))?;

            let command = current.as_mut().ok_or(ParseError::EmptyCommand)?;

            match redirection {
                RedirectionKind::Stdin => command.redirections.stdin = Some(target),
                RedirectionKind::Stdout { append } => {
                    command.redirections.stdout = Some(OutputRedirection { target, append });
                }
            }

            continue;
        }

        match token {
            Token::Word(_)
            | Token::StringLiteral(_)
            | Token::IntLiteral(_)
            | Token::BoolLiteral(_) => {
                if current.is_none() {
                    let name = token_to_command_name(&token)
                        .expect("word and literal tokens map to command names");
                    current = Some(ParsedCommand {
                        name,
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    });
                    continue;
                }

                let expression = token_to_expression(&token)
                    .expect("word and literal tokens map to expressions");

                match &mut current {
                    Some(command) => command.args.push(expression),
                    None => unreachable!("current command was initialized above"),
                }
            }

            Token::Pipe => match current.take() {
                Some(command) => {
                    if command.redirections.stdout.is_some() {
                        return Err(ParseError::UnsupportedRedirection(Token::RedirectOut));
                    }

                    commands.push(command);
                }
                None => return Err(ParseError::UnexpectedToken(Token::Pipe)),
            },

            Token::RedirectIn => {
                ensure_current_command(&current, Token::RedirectIn)?;
                pending_redirection = Some(RedirectionKind::Stdin);
            }

            Token::RedirectOut => {
                ensure_current_command(&current, Token::RedirectOut)?;
                pending_redirection = Some(RedirectionKind::Stdout { append: false });
            }

            Token::RedirectAppend => {
                ensure_current_command(&current, Token::RedirectAppend)?;
                pending_redirection = Some(RedirectionKind::Stdout { append: true });
            }

            token => {
                return Err(ParseError::UnexpectedToken(token));
            }
        }
    }

    if pending_redirection.is_some() {
        return Err(ParseError::MissingRedirectionTarget);
    }

    match current {
        Some(command) => commands.push(command),
        None => return Err(ParseError::UnexpectedToken(Token::Pipe)),
    }

    Ok(Pipeline { commands })
}

#[derive(Debug)]
enum RedirectionKind {
    Stdin,
    Stdout { append: bool },
}

fn ensure_current_command(current: &Option<ParsedCommand>, token: Token) -> Result<(), ParseError> {
    if current.is_some() {
        Ok(())
    } else {
        Err(ParseError::UnexpectedToken(token))
    }
}

fn is_valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();

    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_reserved_name(value: &str) -> bool {
    matches!(value, "status")
}

fn token_to_expression(token: &Token) -> Option<Expression> {
    match token {
        Token::Word(value) => Some(word_to_expression(value.clone())),
        Token::StringLiteral(value) => Some(Expression::Literal(Value::String(value.clone()))),
        Token::IntLiteral(value) => Some(Expression::Literal(Value::Int(*value))),
        Token::BoolLiteral(value) => Some(Expression::Literal(Value::Bool(*value))),
        _ => None,
    }
}

fn word_to_expression(value: String) -> Expression {
    if value == "status" {
        return Expression::Status;
    }

    if let Some(name) = value.strip_prefix('@') {
        return Expression::EnvironmentVariable(name.into());
    }

    if let Some(name) = value.strip_prefix("env.") {
        return Expression::EnvironmentVariable(name.into());
    }

    Expression::Identifier(value)
}

fn token_to_redirection_target(token: &Token) -> Option<String> {
    match token {
        Token::Word(value) | Token::StringLiteral(value) => Some(value.clone()),
        Token::IntLiteral(value) => Some(value.to_string()),
        Token::BoolLiteral(value) => Some(value.to_string()),
        _ => None,
    }
}

fn token_to_command_name(token: &Token) -> Option<String> {
    match token {
        Token::Word(value) | Token::StringLiteral(value) => Some(value.clone()),
        Token::IntLiteral(value) => Some(value.to_string()),
        Token::BoolLiteral(value) => Some(value.to_string()),
        _ => None,
    }
}

pub fn format_error(error: &ParseError) -> String {
    match error {
        ParseError::ReservedName(name) => {
            format!("cannot assign to reserved name '{name}'")
        }
        _ => format!("parse error: {error:?}"),
    }
}

#[test]
fn parses_simple_command() {
    let result = parse("print hello").unwrap();

    assert_eq!(
        result,
        ParsedInput::Pipeline(Pipeline {
            commands: vec![ParsedCommand {
                name: "print".into(),
                args: vec!["hello".into()],
                redirections: Redirections::default(),
            }],
        })
    );
}

#[test]
fn parses_quoted_command_argument_as_literal() {
    let result = parse(r#"print "project""#);

    assert_eq!(
        result,
        Ok(ParsedInput::Pipeline(Pipeline {
            commands: vec![ParsedCommand {
                name: "print".into(),
                args: vec![Value::String("project".into()).into()],
                redirections: Redirections::default(),
            }],
        }))
    );
}

#[test]
fn parses_pipeline() {
    let result = parse("ls -la | grep rs | sort").unwrap();

    assert_eq!(
        result,
        ParsedInput::Pipeline(Pipeline {
            commands: vec![
                ParsedCommand {
                    name: "ls".into(),
                    args: vec!["-la".into()],
                    redirections: Redirections::default(),
                },
                ParsedCommand {
                    name: "grep".into(),
                    args: vec!["rs".into()],
                    redirections: Redirections::default(),
                },
                ParsedCommand {
                    name: "sort".into(),
                    args: Vec::new(),
                    redirections: Redirections::default(),
                },
            ],
        })
    );
}

#[test]
fn parses_boolean_words_as_command_names_in_command_position() {
    let result = parse("true | false").unwrap();

    assert_eq!(
        result,
        ParsedInput::Pipeline(Pipeline {
            commands: vec![
                ParsedCommand {
                    name: "true".into(),
                    args: Vec::new(),
                    redirections: Redirections::default(),
                },
                ParsedCommand {
                    name: "false".into(),
                    args: Vec::new(),
                    redirections: Redirections::default(),
                },
            ],
        })
    );
}

#[test]
fn parses_output_redirection() {
    let result = parse("print hello > out.txt").unwrap();

    assert_eq!(
        result,
        ParsedInput::Pipeline(Pipeline {
            commands: vec![ParsedCommand {
                name: "print".into(),
                args: vec!["hello".into()],
                redirections: Redirections {
                    stdin: None,
                    stdout: Some(OutputRedirection {
                        target: "out.txt".into(),
                        append: false,
                    }),
                },
            }],
        })
    );
}

#[test]
fn parses_append_redirection() {
    let result = parse("print hello >> out.txt").unwrap();
    let ParsedInput::Pipeline(result) = result else {
        panic!("expected pipeline");
    };

    assert_eq!(
        result.commands[0].redirections.stdout,
        Some(OutputRedirection {
            target: "out.txt".into(),
            append: true,
        })
    );
}

#[test]
fn parses_input_and_output_redirection() {
    let result = parse("grep crab < input.txt > output.txt").unwrap();

    assert_eq!(
        result,
        ParsedInput::Pipeline(Pipeline {
            commands: vec![ParsedCommand {
                name: "grep".into(),
                args: vec!["crab".into()],
                redirections: Redirections {
                    stdin: Some("input.txt".into()),
                    stdout: Some(OutputRedirection {
                        target: "output.txt".into(),
                        append: false,
                    }),
                },
            }],
        })
    );
}

#[test]
fn parses_pipeline_output_redirection() {
    let result = parse("ls | grep rs > results.txt").unwrap();
    let ParsedInput::Pipeline(result) = result else {
        panic!("expected pipeline");
    };

    assert_eq!(
        result.commands[1].redirections.stdout,
        Some(OutputRedirection {
            target: "results.txt".into(),
            append: false,
        })
    );
}

#[test]
fn parses_native_variable_assignment() {
    let result = parse(r#"let project = "crbsh""#);

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "project".into(),
            type_annotation: None,
            value: Value::String("crbsh".into()).into(),
        })
    );
}

#[test]
fn parses_native_integer_assignment() {
    let result = parse("let retries = 3");

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "retries".into(),
            type_annotation: None,
            value: Value::Int(3).into(),
        })
    );
}

#[test]
fn parses_native_bool_assignment() {
    let result = parse("let is_active = true");

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "is_active".into(),
            type_annotation: None,
            value: Value::Bool(true).into(),
        })
    );
}

#[test]
fn parses_typed_native_variable_assignment() {
    let result = parse(r#"let project: string = "crbsh""#);

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "project".into(),
            type_annotation: Some(TypeName::String),
            value: Value::String("crbsh".into()).into(),
        })
    );
}

#[test]
fn parses_native_reassignment() {
    let result = parse("retries = 5");

    assert_eq!(
        result,
        Ok(ParsedInput::Assignment {
            name: "retries".into(),
            value: Value::Int(5).into(),
        })
    );
}

#[test]
fn parses_environment_assignment() {
    let result = parse(r#"env.RUST_LOG = "debug""#);

    assert_eq!(
        result,
        Ok(ParsedInput::EnvironmentAssignment {
            name: "RUST_LOG".into(),
            value: Value::String("debug".into()).into(),
        })
    );
}

#[test]
fn rejects_environment_namespace_as_command() {
    let result = parse("env.");

    assert_eq!(
        result,
        Err(ParseError::UnexpectedToken(Token::Word("env.".into())))
    );
}

#[test]
fn rejects_missing_assignment_value() {
    let result = parse("let project =");

    assert_eq!(result, Err(ParseError::MissingAssignmentValue));
}

#[test]
fn rejects_assignment_to_status() {
    let result = parse("let status = 5");

    assert_eq!(result, Err(ParseError::ReservedName("status".into())));
}

#[test]
fn rejects_missing_redirection_target() {
    let result = parse("print hello >");

    assert_eq!(result, Err(ParseError::MissingRedirectionTarget));
}

#[test]
fn rejects_pipe_as_redirection_target() {
    let result = parse("print hello > | grep hello");

    assert_eq!(result, Err(ParseError::UnexpectedToken(Token::Pipe)));
}

#[test]
fn rejects_output_redirection_before_pipe() {
    let result = parse("print hello > out.txt | grep hello");

    assert_eq!(
        result,
        Err(ParseError::UnsupportedRedirection(Token::RedirectOut))
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
