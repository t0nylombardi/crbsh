use std::fmt;

use crate::lexer::{Token, TokenizeError};

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Tokenize(TokenizeError),
    EmptyCommand,
    ExpectedAssignmentOperator,
    InvalidEnvironmentName(String),
    InvalidTypeName(String),
    InvalidVariableName(String),
    InvalidIterable(String),
    MissingBlockStart,
    MissingAssignmentValue,
    MissingFunctionName,
    MissingParameterList,
    MissingParameterType(String),
    MissingBlockEnd,
    MissingRedirectionTarget,
    MissingReturnType,
    MissingMatchArrow,
    MissingMatchPattern,
    NonExhaustiveMatchExpression,
    ReservedName(String),
    UnsupportedRedirection(Token),
    UnexpectedToken(Token),
}

impl From<TokenizeError> for ParseError {
    fn from(error: TokenizeError) -> Self {
        Self::Tokenize(error)
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tokenize(error) => write!(formatter, "{error}"),
            Self::EmptyCommand => write!(formatter, "empty command"),
            Self::ExpectedAssignmentOperator => write!(formatter, "expected '='"),
            Self::InvalidEnvironmentName(name) => {
                write!(formatter, "invalid environment variable name '{name}'")
            }
            Self::InvalidTypeName(name) => write!(formatter, "invalid type name '{name}'"),
            Self::InvalidVariableName(name) => write!(formatter, "invalid variable name '{name}'"),
            Self::InvalidIterable(value) => write!(formatter, "invalid iterable '{value}'"),
            Self::MissingBlockStart => write!(formatter, "missing block start '{{'"),
            Self::MissingAssignmentValue => write!(formatter, "missing assignment value"),
            Self::MissingFunctionName => write!(formatter, "missing function name"),
            Self::MissingParameterList => write!(formatter, "missing parameter list"),
            Self::MissingParameterType(name) => {
                write!(formatter, "missing type for parameter '{name}'")
            }
            Self::MissingBlockEnd => write!(formatter, "missing block end '}}'"),
            Self::MissingRedirectionTarget => write!(formatter, "missing redirection target"),
            Self::MissingReturnType => write!(formatter, "missing return type"),
            Self::MissingMatchArrow => write!(formatter, "missing match arrow '=>'"),
            Self::MissingMatchPattern => write!(formatter, "missing match pattern"),
            Self::NonExhaustiveMatchExpression => {
                write!(formatter, "match expression requires a wildcard '_' arm")
            }
            Self::ReservedName(name) => {
                write!(formatter, "cannot assign to reserved name '{name}'")
            }
            Self::UnsupportedRedirection(token) => write!(
                formatter,
                "unsupported redirection near {}",
                token_description(token)
            ),
            Self::UnexpectedToken(token) => {
                write!(formatter, "unexpected token {}", token_description(token))
            }
        }
    }
}

pub fn format_error(error: &ParseError) -> String {
    error.to_string()
}

fn token_description(token: &Token) -> String {
    match token {
        Token::Word(value) | Token::StringLiteral(value) => format!("'{value}'"),
        Token::IntLiteral(value) => format!("'{value}'"),
        Token::BoolLiteral(value) => format!("'{value}'"),
        Token::Assign => "'='".into(),
        Token::Equal => "'=='".into(),
        Token::NotEqual => "'!='".into(),
        Token::FatArrow => "'=>'".into(),
        Token::Arrow => "'->'".into(),
        Token::Colon => "':'".into(),
        Token::Comma => "','".into(),
        Token::Plus => "'+'".into(),
        Token::Minus => "'-'".into(),
        Token::Star => "'*'".into(),
        Token::Slash => "'/'".into(),
        Token::LeftParen => "'('".into(),
        Token::RightParen => "')'".into(),
        Token::LeftBracket => "'['".into(),
        Token::RightBracket => "']'".into(),
        Token::LessEqual => "'<='".into(),
        Token::GreaterEqual => "'>='".into(),
        Token::LeftBrace => "'{'".into(),
        Token::RightBrace => "'}'".into(),
        Token::Wildcard => "'_'".into(),
        Token::Pipe => "'|'".into(),
        Token::AndIf => "'&&'".into(),
        Token::OrIf => "'||'".into(),
        Token::RedirectOut => "'>'".into(),
        Token::RedirectAppend => "'>>'".into(),
        Token::RedirectIn => "'<'".into(),
        Token::Background => "'&'".into(),
    }
}
