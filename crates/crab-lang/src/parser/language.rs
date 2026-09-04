use std::collections::BTreeMap;

use crate::runtime::{TypeName, Value};

use super::ParsedInput;

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
    Construct {
        type_name: String,
        fields: BTreeMap<String, Expression>,
    },
    EnumVariant {
        enum_name: String,
        variant: String,
        payload: Option<Box<Expression>>,
    },
    List(Vec<Expression>),
    Index {
        target: Box<Expression>,
        index: Box<Expression>,
    },
    Field {
        target: Box<Expression>,
        name: String,
    },
    Match {
        value: Box<Expression>,
        arms: Vec<MatchExpressionArm>,
    },
    Len(Box<Expression>),
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchExpressionArm {
    pub pattern: MatchPattern,
    pub value: Expression,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDefinition {
    pub params: Vec<FunctionParam>,
    pub return_type: Option<TypeName>,
    pub body: Vec<ParsedInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParam {
    pub name: String,
    pub type_annotation: Option<TypeName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefinition {
    pub fields: BTreeMap<String, TypeName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDefinition {
    pub variants: BTreeMap<String, Option<TypeName>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Iterable {
    Range {
        start: Expression,
        end: Expression,
        inclusive: bool,
    },
    Glob(String),
    Expression(Expression),
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
    EnumVariant {
        enum_name: String,
        variant: String,
        binding: Option<String>,
    },
}
