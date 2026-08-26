use crate::lexer::Token;
use crate::runtime::Value;

use super::ast::{BinaryOperator, Expression};
use super::error::ParseError;

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

        if let Token::Word(name) = token
            && matches!(self.peek(), Some(Token::LeftParen))
        {
            self.position += 1;
            let expression = self.parse_call(name.clone())?;
            return self.parse_postfix(expression);
        }

        let expression =
            token_to_expression(token).ok_or_else(|| ParseError::UnexpectedToken(token.clone()))?;
        self.parse_postfix(expression)
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

            if let Expression::Identifier(name) = &expression
                && let Some(target) = name.strip_suffix(".len")
                && !target.is_empty()
            {
                expression = Expression::Len(Box::new(word_to_expression(target.into())));
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
