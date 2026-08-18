use crate::tokens::{Token, TokenizeError, tokenize};
use crate::value::{TypeName, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
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
    Call {
        name: String,
        args: Vec<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl BinaryOperator {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redirections {
    pub stdin: Option<String>,
    pub stdout: Option<OutputRedirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRedirection {
    pub target: String,
    pub append: bool,
}

impl Redirections {
    pub fn is_empty(&self) -> bool {
        self.stdin.is_none() && self.stdout.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<ParsedCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedInput {
    Pipeline(Pipeline),
    FunctionDefinition {
        name: String,
        definition: FunctionDefinition,
    },
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
    Return {
        value: Option<Expression>,
    },
    Break,
    Continue,
    If {
        branches: Vec<IfBranch>,
        else_body: Option<Vec<ParsedInput>>,
    },
    Match {
        value: Expression,
        arms: Vec<MatchArm>,
    },
    While {
        condition: Expression,
        body: Vec<ParsedInput>,
    },
    For {
        name: String,
        iterable: Iterable,
        body: Vec<ParsedInput>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDefinition {
    pub params: Vec<FunctionParam>,
    pub return_type: Option<TypeName>,
    pub body: Vec<ParsedInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParam {
    pub name: String,
    pub type_name: TypeName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Iterable {
    Range {
        start: Expression,
        end: Expression,
        inclusive: bool,
    },
    Glob(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfBranch {
    pub condition: Expression,
    pub body: Vec<ParsedInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: ParsedInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchPattern {
    Literal(Value),
    Identifier(String),
    Status,
    Wildcard,
}

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
    MissingRedirectionTarget,
    MissingReturnType,
    MissingMatchArrow,
    MissingMatchPattern,
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
    let input = input.trim();

    if input.starts_with("if ") {
        return parse_if(input);
    }

    if input.starts_with("match ") {
        return parse_match(input);
    }

    if input.starts_with("while ") {
        return parse_while(input);
    }

    if input.starts_with("for ") {
        return parse_for(input);
    }

    if input.starts_with("fn ") {
        return parse_function(input);
    }

    let tokens = tokenize(input)?;

    if tokens.is_empty() {
        return Err(ParseError::EmptyCommand);
    }

    match tokens.as_slice() {
        [Token::Word(keyword), rest @ ..] if keyword == "let" => {
            return parse_let(rest);
        }
        [Token::Word(keyword)] if keyword == "break" => return Ok(ParsedInput::Break),
        [Token::Word(keyword)] if keyword == "continue" => return Ok(ParsedInput::Continue),
        [Token::Word(keyword)] if keyword == "return" => {
            return Ok(ParsedInput::Return { value: None });
        }
        [Token::Word(keyword), rest @ ..] if keyword == "return" => {
            return Ok(ParsedInput::Return {
                value: Some(parse_expression(rest)?),
            });
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

fn parse_if(input: &str) -> Result<ParsedInput, ParseError> {
    let lines = normalized_lines(input);
    let mut branches = Vec::new();
    let mut else_body = None;
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];

        if let Some(condition_source) = line
            .strip_prefix("if ")
            .or_else(|| line.strip_prefix("} else if "))
        {
            let condition_source = condition_source
                .strip_suffix('{')
                .map(str::trim)
                .ok_or(ParseError::MissingBlockStart)?;
            let (body, next_index) = parse_block_body(&lines, index + 1)?;

            branches.push(IfBranch {
                condition: parse_expression_from_source(condition_source)?,
                body,
            });
            index = next_index;
            continue;
        }

        if line == "} else {" {
            let (body, next_index) = parse_block_body(&lines, index + 1)?;
            else_body = Some(body);
            index = next_index;
            continue;
        }

        if line == "}" {
            index += 1;
            continue;
        }

        return Err(ParseError::UnexpectedToken(Token::Word(line.into())));
    }

    Ok(ParsedInput::If {
        branches,
        else_body,
    })
}

fn parse_match(input: &str) -> Result<ParsedInput, ParseError> {
    let lines = normalized_lines(input);
    let Some(header) = lines.first() else {
        return Err(ParseError::EmptyCommand);
    };

    let value_source = header
        .strip_prefix("match ")
        .and_then(|line| line.strip_suffix('{'))
        .map(str::trim)
        .ok_or(ParseError::MissingBlockStart)?;
    let mut arms = Vec::new();

    for line in lines.iter().skip(1) {
        if *line == "}" {
            continue;
        }

        let Some((pattern, body)) = line.split_once("=>") else {
            return Err(ParseError::MissingMatchArrow);
        };

        let pattern = parse_match_pattern(pattern.trim())?;
        let body = parse(body.trim())?;

        arms.push(MatchArm { pattern, body });
    }

    Ok(ParsedInput::Match {
        value: parse_expression_from_source(value_source)?,
        arms,
    })
}

fn parse_while(input: &str) -> Result<ParsedInput, ParseError> {
    let lines = normalized_lines(input);
    let Some(header) = lines.first() else {
        return Err(ParseError::EmptyCommand);
    };

    let condition_source = header
        .strip_prefix("while ")
        .and_then(|line| line.strip_suffix('{'))
        .map(str::trim)
        .ok_or(ParseError::MissingBlockStart)?;
    let (body, _) = parse_block_body(&lines, 1)?;

    Ok(ParsedInput::While {
        condition: parse_expression_from_source(condition_source)?,
        body,
    })
}

fn parse_for(input: &str) -> Result<ParsedInput, ParseError> {
    let lines = normalized_lines(input);
    let Some(header) = lines.first() else {
        return Err(ParseError::EmptyCommand);
    };

    let header = header
        .strip_prefix("for ")
        .and_then(|line| line.strip_suffix('{'))
        .map(str::trim)
        .ok_or(ParseError::MissingBlockStart)?;
    let Some((name, iterable_source)) = header.split_once(" in ") else {
        return Err(ParseError::InvalidIterable(header.into()));
    };
    let name = name.trim();

    if !is_valid_identifier(name) {
        return Err(ParseError::InvalidVariableName(name.into()));
    }

    if is_reserved_name(name) {
        return Err(ParseError::ReservedName(name.into()));
    }

    let (body, _) = parse_block_body(&lines, 1)?;

    Ok(ParsedInput::For {
        name: name.into(),
        iterable: parse_iterable(iterable_source.trim())?,
        body,
    })
}

fn parse_function(input: &str) -> Result<ParsedInput, ParseError> {
    let lines = normalized_lines(input);
    let Some(header) = lines.first() else {
        return Err(ParseError::EmptyCommand);
    };

    let header = header
        .strip_prefix("fn ")
        .and_then(|line| line.strip_suffix('{'))
        .map(str::trim)
        .ok_or(ParseError::MissingBlockStart)?;
    let open_paren = header.find('(').ok_or(ParseError::MissingParameterList)?;
    let close_paren = header.rfind(')').ok_or(ParseError::MissingParameterList)?;

    if close_paren < open_paren {
        return Err(ParseError::MissingParameterList);
    }

    let name = header[..open_paren].trim();

    if name.is_empty() {
        return Err(ParseError::MissingFunctionName);
    }

    if !is_valid_identifier(name) {
        return Err(ParseError::InvalidVariableName(name.into()));
    }

    if is_reserved_name(name) {
        return Err(ParseError::ReservedName(name.into()));
    }

    let params = parse_function_params(&header[open_paren + 1..close_paren])?;
    let return_type = parse_function_return_type(header[close_paren + 1..].trim())?;
    let (body, _) = parse_block_body(&lines, 1)?;

    Ok(ParsedInput::FunctionDefinition {
        name: name.into(),
        definition: FunctionDefinition {
            params,
            return_type,
            body,
        },
    })
}

fn parse_function_params(input: &str) -> Result<Vec<FunctionParam>, ParseError> {
    let input = input.trim();

    if input.is_empty() {
        return Ok(Vec::new());
    }

    input
        .split(',')
        .map(|param| {
            let Some((name, type_name)) = param.trim().split_once(':') else {
                return Err(ParseError::MissingParameterList);
            };
            let name = name.trim();
            let type_name = type_name.trim();

            if !is_valid_identifier(name) {
                return Err(ParseError::InvalidVariableName(name.into()));
            }

            if is_reserved_name(name) {
                return Err(ParseError::ReservedName(name.into()));
            }

            let Some(type_name) = TypeName::parse(type_name) else {
                return Err(ParseError::InvalidTypeName(type_name.into()));
            };

            Ok(FunctionParam {
                name: name.into(),
                type_name,
            })
        })
        .collect()
}

fn parse_function_return_type(input: &str) -> Result<Option<TypeName>, ParseError> {
    if input.is_empty() {
        return Ok(None);
    }

    let Some(type_name) = input.strip_prefix("->").map(str::trim) else {
        return Err(ParseError::MissingReturnType);
    };

    let Some(type_name) = TypeName::parse(type_name) else {
        return Err(ParseError::InvalidTypeName(type_name.into()));
    };

    Ok(Some(type_name))
}

fn parse_iterable(input: &str) -> Result<Iterable, ParseError> {
    if let Some((start, end)) = input.split_once("..=") {
        return Ok(Iterable::Range {
            start: parse_expression_from_source(start.trim())?,
            end: parse_expression_from_source(end.trim())?,
            inclusive: true,
        });
    }

    if let Some((start, end)) = input.split_once("..") {
        return Ok(Iterable::Range {
            start: parse_expression_from_source(start.trim())?,
            end: parse_expression_from_source(end.trim())?,
            inclusive: false,
        });
    }

    if input.contains('*') {
        return Ok(Iterable::Glob(input.into()));
    }

    Err(ParseError::InvalidIterable(input.into()))
}

fn parse_block_body(
    lines: &[&str],
    start_index: usize,
) -> Result<(Vec<ParsedInput>, usize), ParseError> {
    let mut body = Vec::new();
    let mut index = start_index;

    while index < lines.len() {
        let line = lines[index];

        if line == "}" || line.starts_with("} else") {
            return Ok((body, index));
        }

        if starts_block_statement(line) {
            let (statement, next_index) = collect_block_statement(lines, index);

            body.push(parse(&statement)?);
            index = next_index;
        } else {
            body.push(parse(line)?);
            index += 1;
        }
    }

    Ok((body, index))
}

fn starts_block_statement(line: &str) -> bool {
    (line.starts_with("if ")
        || line.starts_with("match ")
        || line.starts_with("while ")
        || line.starts_with("for ")
        || line.starts_with("fn "))
        && line.ends_with('{')
}

fn collect_block_statement(lines: &[&str], start_index: usize) -> (String, usize) {
    let mut statement = String::new();
    let mut balance = 0;
    let mut index = start_index;

    while index < lines.len() {
        let line = lines[index];

        if !statement.is_empty() {
            statement.push('\n');
        }
        statement.push_str(line);
        balance += brace_delta(line);
        index += 1;

        if balance <= 0 {
            break;
        }
    }

    (statement, index)
}

fn brace_delta(input: &str) -> i32 {
    let mut balance = 0;
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quotes => escaped = true,
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes => in_double_quotes = !in_double_quotes,
            '{' if !in_single_quotes && !in_double_quotes => balance += 1,
            '}' if !in_single_quotes && !in_double_quotes => balance -= 1,
            _ => {}
        }
    }

    balance
}

fn normalized_lines(input: &str) -> Vec<&str> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn parse_expression_from_source(input: &str) -> Result<Expression, ParseError> {
    let tokens = tokenize(input)?;

    parse_expression(&tokens)
}

fn parse_match_pattern(input: &str) -> Result<MatchPattern, ParseError> {
    let tokens = tokenize(input)?;

    let [token] = tokens.as_slice() else {
        if tokens.is_empty() {
            return Err(ParseError::MissingMatchPattern);
        }

        return Err(ParseError::UnexpectedToken(
            tokens
                .first()
                .cloned()
                .unwrap_or_else(|| Token::Word(input.into())),
        ));
    };

    match token {
        Token::Wildcard => Ok(MatchPattern::Wildcard),
        Token::StringLiteral(value) => Ok(MatchPattern::Literal(Value::String(value.clone()))),
        Token::IntLiteral(value) => Ok(MatchPattern::Literal(Value::Int(*value))),
        Token::BoolLiteral(value) => Ok(MatchPattern::Literal(Value::Bool(*value))),
        Token::Word(value) if value == "status" => Ok(MatchPattern::Status),
        Token::Word(value) => Ok(MatchPattern::Identifier(value.clone())),
        token => Err(ParseError::UnexpectedToken(token.clone())),
    }
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
    let [Token::Assign, rest @ ..] = tokens else {
        return Err(ParseError::ExpectedAssignmentOperator);
    };

    parse_expression(rest)
}

fn parse_assignment_value(tokens: &[Token]) -> Result<Expression, ParseError> {
    parse_expression(tokens)
}

fn parse_expression(tokens: &[Token]) -> Result<Expression, ParseError> {
    if tokens.is_empty() {
        return Err(ParseError::MissingAssignmentValue);
    }

    let mut parser = ExpressionParser::new(tokens);
    let expression = parser.parse_equality()?;

    if let Some(token) = parser.peek() {
        return Err(ParseError::UnexpectedToken(token.clone()));
    }

    Ok(expression)
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
            | Token::BoolLiteral(_)
            | Token::Assign
            | Token::Equal
            | Token::NotEqual
            | Token::Colon
            | Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
            | Token::LessEqual
            | Token::GreaterEqual => {
                if current.is_none() {
                    let name = token_to_command_name(&token)
                        .expect("word, literal, and operator tokens map to command names");
                    current = Some(ParsedCommand {
                        name,
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    });
                    continue;
                }

                let expression = token_to_command_argument(&token)
                    .expect("word, literal, and operator tokens map to command arguments");

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

struct ExpressionParser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn parse_equality(&mut self) -> Result<Expression, ParseError> {
        let mut expression = self.parse_comparison()?;

        while let Some(operator) = self.match_equality_operator() {
            let right = self.parse_comparison()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_comparison(&mut self) -> Result<Expression, ParseError> {
        let mut expression = self.parse_term()?;

        while let Some(operator) = self.match_comparison_operator() {
            let right = self.parse_term()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_term(&mut self) -> Result<Expression, ParseError> {
        let mut expression = self.parse_factor()?;

        while let Some(operator) = self.match_term_operator() {
            let right = self.parse_factor()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_factor(&mut self) -> Result<Expression, ParseError> {
        let mut expression = self.parse_primary()?;

        while let Some(operator) = self.match_factor_operator() {
            let right = self.parse_primary()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }

        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<Expression, ParseError> {
        let Some(token) = self.advance() else {
            return Err(ParseError::MissingAssignmentValue);
        };

        if let Token::Word(name) = token
            && matches!(self.peek(), Some(Token::LeftParen))
        {
            self.position += 1;
            return self.parse_call(name.clone());
        }

        token_to_expression(token).ok_or_else(|| ParseError::UnexpectedToken(token.clone()))
    }

    fn parse_call(&mut self, name: String) -> Result<Expression, ParseError> {
        let mut args = Vec::new();

        if matches!(self.peek(), Some(Token::RightParen)) {
            self.position += 1;
            return Ok(Expression::Call { name, args });
        }

        loop {
            args.push(self.parse_equality()?);

            match self.peek() {
                Some(Token::Comma) => {
                    self.position += 1;
                }
                Some(Token::RightParen) => {
                    self.position += 1;
                    break;
                }
                Some(token) => return Err(ParseError::UnexpectedToken(token.clone())),
                None => return Err(ParseError::UnexpectedToken(Token::LeftParen)),
            }
        }

        Ok(Expression::Call { name, args })
    }

    fn match_equality_operator(&mut self) -> Option<BinaryOperator> {
        match self.peek() {
            Some(Token::Equal) => {
                self.position += 1;
                Some(BinaryOperator::Equal)
            }
            Some(Token::NotEqual) => {
                self.position += 1;
                Some(BinaryOperator::NotEqual)
            }
            _ => None,
        }
    }

    fn match_comparison_operator(&mut self) -> Option<BinaryOperator> {
        match self.peek() {
            Some(Token::RedirectIn) => {
                self.position += 1;
                Some(BinaryOperator::Less)
            }
            Some(Token::LessEqual) => {
                self.position += 1;
                Some(BinaryOperator::LessEqual)
            }
            Some(Token::RedirectOut) => {
                self.position += 1;
                Some(BinaryOperator::Greater)
            }
            Some(Token::GreaterEqual) => {
                self.position += 1;
                Some(BinaryOperator::GreaterEqual)
            }
            _ => None,
        }
    }

    fn match_term_operator(&mut self) -> Option<BinaryOperator> {
        match self.peek() {
            Some(Token::Plus) => {
                self.position += 1;
                Some(BinaryOperator::Add)
            }
            Some(Token::Minus) => {
                self.position += 1;
                Some(BinaryOperator::Subtract)
            }
            _ => None,
        }
    }

    fn match_factor_operator(&mut self) -> Option<BinaryOperator> {
        match self.peek() {
            Some(Token::Star) => {
                self.position += 1;
                Some(BinaryOperator::Multiply)
            }
            Some(Token::Slash) => {
                self.position += 1;
                Some(BinaryOperator::Divide)
            }
            _ => None,
        }
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<&'a Token> {
        let token = self.peek()?;
        self.position += 1;
        Some(token)
    }
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
        token => token_to_operator_string(token).map(String::from),
    }
}

fn token_to_command_argument(token: &Token) -> Option<Expression> {
    token_to_expression(token).or_else(|| {
        token_to_operator_string(token)
            .map(|value| Expression::Literal(Value::String(value.into())))
    })
}

fn token_to_operator_string(token: &Token) -> Option<&'static str> {
    match token {
        Token::Assign => Some("="),
        Token::Equal => Some("=="),
        Token::NotEqual => Some("!="),
        Token::Colon => Some(":"),
        Token::Plus => Some("+"),
        Token::Minus => Some("-"),
        Token::Star => Some("*"),
        Token::Slash => Some("/"),
        Token::LessEqual => Some("<="),
        Token::GreaterEqual => Some(">="),
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
fn parses_addition_expression() {
    let result = parse("let next = retries + 1");

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "next".into(),
            type_annotation: None,
            value: Expression::Binary {
                left: Box::new(Expression::Identifier("retries".into())),
                operator: BinaryOperator::Add,
                right: Box::new(Value::Int(1).into()),
            },
        })
    );
}

#[test]
fn parses_comparison_expression() {
    let result = parse("let active = retries < 5");

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "active".into(),
            type_annotation: None,
            value: Expression::Binary {
                left: Box::new(Expression::Identifier("retries".into())),
                operator: BinaryOperator::Less,
                right: Box::new(Value::Int(5).into()),
            },
        })
    );
}

#[test]
fn parses_equality_expression() {
    let result = parse("let ready = active == true");

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "ready".into(),
            type_annotation: None,
            value: Expression::Binary {
                left: Box::new(Expression::Identifier("active".into())),
                operator: BinaryOperator::Equal,
                right: Box::new(Value::Bool(true).into()),
            },
        })
    );
}

#[test]
fn parses_if_else_if_else_block() {
    let result = parse(
        r#"
if condition {
    print "yep"
} else if other_condition {
    print "maybe"
} else {
    print "nope"
}
"#,
    );

    assert_eq!(
        result,
        Ok(ParsedInput::If {
            branches: vec![
                IfBranch {
                    condition: Expression::Identifier("condition".into()),
                    body: vec![ParsedInput::Pipeline(Pipeline {
                        commands: vec![ParsedCommand {
                            name: "print".into(),
                            args: vec![Value::String("yep".into()).into()],
                            redirections: Redirections::default(),
                        }],
                    })],
                },
                IfBranch {
                    condition: Expression::Identifier("other_condition".into()),
                    body: vec![ParsedInput::Pipeline(Pipeline {
                        commands: vec![ParsedCommand {
                            name: "print".into(),
                            args: vec![Value::String("maybe".into()).into()],
                            redirections: Redirections::default(),
                        }],
                    })],
                },
            ],
            else_body: Some(vec![ParsedInput::Pipeline(Pipeline {
                commands: vec![ParsedCommand {
                    name: "print".into(),
                    args: vec![Value::String("nope".into()).into()],
                    redirections: Redirections::default(),
                }],
            })]),
        })
    );
}

#[test]
fn parses_status_match_block() {
    let result = parse(
        r#"
match status {
    0 => print "success"
    1 => print "failed"
    _ => print "something weird happened"
}
"#,
    )
    .unwrap();

    let ParsedInput::Match { value, arms } = result else {
        panic!("expected match");
    };

    assert_eq!(value, Expression::Status);
    assert_eq!(arms.len(), 3);
    assert_eq!(arms[0].pattern, MatchPattern::Literal(Value::Int(0)));
    assert_eq!(arms[1].pattern, MatchPattern::Literal(Value::Int(1)));
    assert_eq!(arms[2].pattern, MatchPattern::Wildcard);
}

#[test]
fn parses_native_value_match_block() {
    let result = parse(
        r#"
match environment {
    "development" => print "dev mode"
    "production" => print "prod mode"
    _ => print "unknown environment"
}
"#,
    )
    .unwrap();

    let ParsedInput::Match { value, arms } = result else {
        panic!("expected match");
    };

    assert_eq!(value, Expression::Identifier("environment".into()));
    assert_eq!(
        arms[0].pattern,
        MatchPattern::Literal(Value::String("development".into()))
    );
    assert_eq!(
        arms[1].pattern,
        MatchPattern::Literal(Value::String("production".into()))
    );
    assert_eq!(arms[2].pattern, MatchPattern::Wildcard);
}

#[test]
fn parses_while_block() {
    let result = parse(
        r#"
while retries < 3 {
    print retries
    retries = retries + 1
}
"#,
    )
    .unwrap();

    let ParsedInput::While { condition, body } = result else {
        panic!("expected while");
    };

    assert_eq!(
        condition,
        Expression::Binary {
            left: Box::new(Expression::Identifier("retries".into())),
            operator: BinaryOperator::Less,
            right: Box::new(Value::Int(3).into()),
        }
    );
    assert_eq!(body.len(), 2);
}

#[test]
fn parses_while_block_with_nested_break() {
    let result = parse(
        r#"
while true {
    print "forever"
    if status != 0 {
        break
    }
}
"#,
    )
    .unwrap();

    let ParsedInput::While { condition, body } = result else {
        panic!("expected while");
    };

    assert_eq!(condition, Value::Bool(true).into());
    assert_eq!(body.len(), 2);
    assert!(matches!(body[1], ParsedInput::If { .. }));
}

#[test]
fn parses_exclusive_for_range() {
    let result = parse(
        r#"
for i in 0..10 {
    print i
}
"#,
    )
    .unwrap();

    let ParsedInput::For {
        name,
        iterable,
        body,
    } = result
    else {
        panic!("expected for");
    };

    assert_eq!(name, "i");
    assert_eq!(
        iterable,
        Iterable::Range {
            start: Value::Int(0).into(),
            end: Value::Int(10).into(),
            inclusive: false,
        }
    );
    assert_eq!(body.len(), 1);
}

#[test]
fn parses_inclusive_for_range() {
    let result = parse(
        r#"
for i in 0..=10 {
    print i
}
"#,
    )
    .unwrap();

    let ParsedInput::For { iterable, .. } = result else {
        panic!("expected for");
    };

    assert_eq!(
        iterable,
        Iterable::Range {
            start: Value::Int(0).into(),
            end: Value::Int(10).into(),
            inclusive: true,
        }
    );
}

#[test]
fn parses_for_glob() {
    let result = parse(
        r#"
for file in *.rs {
    print file
}
"#,
    )
    .unwrap();

    let ParsedInput::For { iterable, .. } = result else {
        panic!("expected for");
    };

    assert_eq!(iterable, Iterable::Glob("*.rs".into()));
}

#[test]
fn parses_function_definition() {
    let result = parse(
        r#"
fn add(a: int, b: int) -> int {
    return a + b
}
"#,
    )
    .unwrap();

    let ParsedInput::FunctionDefinition { name, definition } = result else {
        panic!("expected function definition");
    };

    assert_eq!(name, "add");
    assert_eq!(
        definition.params,
        vec![
            FunctionParam {
                name: "a".into(),
                type_name: TypeName::Int,
            },
            FunctionParam {
                name: "b".into(),
                type_name: TypeName::Int,
            },
        ]
    );
    assert_eq!(definition.return_type, Some(TypeName::Int));
    assert_eq!(definition.body.len(), 1);
}

#[test]
fn parses_function_call_expression() {
    let result = parse("let total = add(2, 3)");

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "total".into(),
            type_annotation: None,
            value: Expression::Call {
                name: "add".into(),
                args: vec![Value::Int(2).into(), Value::Int(3).into()],
            },
        })
    );
}

#[test]
fn parses_return_without_value() {
    let result = parse("return");

    assert_eq!(result, Ok(ParsedInput::Return { value: None }));
}

#[test]
fn parses_arithmetic_precedence() {
    let result = parse("let value = 2 + 3 * 4");

    assert_eq!(
        result,
        Ok(ParsedInput::Let {
            name: "value".into(),
            type_annotation: None,
            value: Expression::Binary {
                left: Box::new(Value::Int(2).into()),
                operator: BinaryOperator::Add,
                right: Box::new(Expression::Binary {
                    left: Box::new(Value::Int(3).into()),
                    operator: BinaryOperator::Multiply,
                    right: Box::new(Value::Int(4).into()),
                }),
            },
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
