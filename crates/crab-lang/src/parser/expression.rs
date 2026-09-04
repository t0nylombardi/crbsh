use crate::lexer::Token;
use crate::runtime::Value;
use std::collections::BTreeMap;

use super::error::ParseError;
use super::language::{BinaryOperator, Expression, MatchExpressionArm, MatchPattern};

pub(super) fn parse_expression(tokens: &[Token]) -> Result<Expression, ParseError> {
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

        if matches!(token, Token::LeftBracket) {
            return self.parse_list();
        }

        if matches!(token, Token::Word(keyword) if keyword == "match") {
            return self.parse_match();
        }

        if let Token::Word(name) = token
            && matches!(self.peek(), Some(Token::LeftParen))
        {
            self.position += 1;
            let expression = self.parse_call(name.clone())?;
            let expression = if let Some((enum_name, variant)) = enum_variant_path(name) {
                let Expression::Call { mut args, .. } = expression else {
                    unreachable!()
                };
                if args.len() == 1 {
                    Expression::EnumVariant {
                        enum_name: enum_name.into(),
                        variant: variant.into(),
                        payload: Some(Box::new(args.remove(0))),
                    }
                } else {
                    Expression::Call {
                        name: name.clone(),
                        args,
                    }
                }
            } else {
                expression
            };
            return self.parse_postfix(expression);
        }

        if let Token::Word(name) = token
            && is_named_type(name)
            && matches!(self.peek(), Some(Token::LeftBrace))
        {
            self.position += 1;
            let expression = self.parse_construction(name.clone())?;
            return self.parse_postfix(expression);
        }

        let expression =
            token_to_expression(token).ok_or_else(|| ParseError::UnexpectedToken(token.clone()))?;
        let expression = match expression {
            Expression::Identifier(path) if enum_variant_path(&path).is_some() => {
                let (enum_name, variant) = enum_variant_path(&path).expect("checked variant path");
                Expression::EnumVariant {
                    enum_name: enum_name.into(),
                    variant: variant.into(),
                    payload: None,
                }
            }
            expression => expression,
        };
        self.parse_postfix(expression)
    }

    fn parse_match(&mut self) -> Result<Expression, ParseError> {
        let value = self.parse_equality()?;
        match self.advance() {
            Some(Token::LeftBrace) => {}
            Some(token) => return Err(ParseError::UnexpectedToken(token.clone())),
            None => return Err(ParseError::MissingBlockStart),
        }

        let mut arms = Vec::new();
        while !matches!(self.peek(), Some(Token::RightBrace)) {
            let pattern = self.parse_match_pattern()?;
            match self.advance() {
                Some(Token::FatArrow) => {}
                Some(token) => return Err(ParseError::UnexpectedToken(token.clone())),
                None => return Err(ParseError::MissingMatchArrow),
            }
            let value = self.parse_equality()?;
            arms.push(MatchExpressionArm { pattern, value });

            if matches!(self.peek(), Some(Token::Comma)) {
                self.position += 1;
            }
        }

        if self.advance().is_none() {
            return Err(ParseError::MissingBlockEnd);
        }
        if !arms
            .iter()
            .any(|arm| matches!(arm.pattern, MatchPattern::Wildcard))
        {
            return Err(ParseError::NonExhaustiveMatchExpression);
        }

        self.parse_postfix(Expression::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn parse_match_pattern(&mut self) -> Result<MatchPattern, ParseError> {
        match self.advance() {
            Some(Token::Wildcard) => Ok(MatchPattern::Wildcard),
            Some(Token::StringLiteral(value)) => {
                Ok(MatchPattern::Literal(Value::String(value.clone())))
            }
            Some(Token::IntLiteral(value)) => Ok(MatchPattern::Literal(Value::Int(*value))),
            Some(Token::BoolLiteral(value)) => Ok(MatchPattern::Literal(Value::Bool(*value))),
            Some(Token::Word(path)) if enum_variant_path(path).is_some() => {
                let (enum_name, variant) = enum_variant_path(path).expect("checked variant path");
                let binding = if matches!(self.peek(), Some(Token::LeftParen)) {
                    self.position += 1;
                    let Some(Token::Word(binding)) = self.advance() else {
                        return Err(ParseError::MissingMatchPattern);
                    };
                    let binding = binding.clone();
                    if !matches!(self.advance(), Some(Token::RightParen)) {
                        return Err(ParseError::MissingMatchPattern);
                    }
                    Some(binding)
                } else {
                    None
                };
                Ok(MatchPattern::EnumVariant {
                    enum_name: enum_name.into(),
                    variant: variant.into(),
                    binding,
                })
            }
            Some(token) => Err(ParseError::UnexpectedToken(token.clone())),
            None => Err(ParseError::MissingMatchPattern),
        }
    }

    fn parse_list(&mut self) -> Result<Expression, ParseError> {
        let mut values = Vec::new();

        if matches!(self.peek(), Some(Token::RightBracket)) {
            self.position += 1;
            return Ok(Expression::List(values));
        }

        loop {
            values.push(self.parse_equality()?);
            match self.peek() {
                Some(Token::Comma) => self.position += 1,
                Some(Token::RightBracket) => {
                    self.position += 1;
                    break;
                }
                Some(token) => return Err(ParseError::UnexpectedToken(token.clone())),
                None => return Err(ParseError::UnexpectedToken(Token::LeftBracket)),
            }
        }

        self.parse_postfix(Expression::List(values))
    }

    fn parse_postfix(&mut self, mut expression: Expression) -> Result<Expression, ParseError> {
        loop {
            if matches!(self.peek(), Some(Token::LeftBracket)) {
                self.position += 1;
                let index = self.parse_equality()?;
                match self.advance() {
                    Some(Token::RightBracket) => {
                        expression = Expression::Index {
                            target: Box::new(expression),
                            index: Box::new(index),
                        };
                    }
                    Some(token) => return Err(ParseError::UnexpectedToken(token.clone())),
                    None => return Err(ParseError::UnexpectedToken(Token::LeftBracket)),
                }
                continue;
            }

            if let Expression::Identifier(path) = &expression
                && let Some((target, member)) = path.rsplit_once('.')
                && !target.is_empty()
                && !member.is_empty()
            {
                let target = Box::new(parse_member_target(target));
                expression = if member == "len" {
                    Expression::Len(target)
                } else {
                    Expression::Field {
                        target,
                        name: member.into(),
                    }
                };
            }
            break;
        }

        Ok(expression)
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

    fn parse_construction(&mut self, type_name: String) -> Result<Expression, ParseError> {
        let mut fields = BTreeMap::new();
        if matches!(self.peek(), Some(Token::RightBrace)) {
            self.position += 1;
            return Ok(Expression::Construct { type_name, fields });
        }

        loop {
            let Some(Token::Word(name)) = self.advance() else {
                return Err(ParseError::InvalidTypeField(String::new()));
            };
            let name = name.clone();
            if !is_field_name(&name) {
                return Err(ParseError::InvalidTypeField(name));
            }
            match self.advance() {
                Some(Token::Colon) => {}
                Some(token) => return Err(ParseError::UnexpectedToken(token.clone())),
                None => return Err(ParseError::MissingAssignmentValue),
            }
            let value = self.parse_equality()?;
            if fields.insert(name.clone(), value).is_some() {
                return Err(ParseError::DuplicateField(name));
            }

            match self.peek() {
                Some(Token::Comma) => self.position += 1,
                Some(Token::RightBrace) => {
                    self.position += 1;
                    break;
                }
                Some(token) => return Err(ParseError::UnexpectedToken(token.clone())),
                None => return Err(ParseError::MissingBlockEnd),
            }
        }

        Ok(Expression::Construct { type_name, fields })
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

fn is_named_type(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|name| name.starts_with(|ch: char| ch.is_ascii_uppercase()))
}

fn is_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub(super) fn enum_variant_path(path: &str) -> Option<(&str, &str)> {
    let (enum_name, variant) = path.rsplit_once("::")?;
    let type_name = enum_name.rsplit("::").next()?;
    (type_name.starts_with(|ch: char| ch.is_ascii_uppercase())
        && variant.starts_with(|ch: char| ch.is_ascii_uppercase()))
    .then_some((enum_name, variant))
}

fn parse_member_target(path: &str) -> Expression {
    let Some((target, field)) = path.split_once('.') else {
        return word_to_expression(path.into());
    };
    let target = Box::new(parse_member_target(target));
    if field == "len" {
        Expression::Len(target)
    } else {
        Expression::Field {
            target,
            name: field.into(),
        }
    }
}

pub(super) fn token_to_expression(token: &Token) -> Option<Expression> {
    match token {
        Token::Word(value) => Some(word_to_expression(value.clone())),
        Token::StringLiteral(value) => Some(Expression::Literal(Value::String(value.clone()))),
        Token::IntLiteral(value) => Some(Expression::Literal(Value::Int(*value))),
        Token::BoolLiteral(value) => Some(Expression::Literal(Value::Bool(*value))),
        _ => None,
    }
}

pub(super) fn word_to_expression(value: String) -> Expression {
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
