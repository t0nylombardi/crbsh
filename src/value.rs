use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
}

impl Value {
    pub fn type_name(&self) -> TypeName {
        match self {
            Self::String(_) => TypeName::String,
            Self::Int(_) => TypeName::Int,
            Self::Bool(_) => TypeName::Bool,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => write!(formatter, "{value}"),
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Bool(value) => write!(formatter, "{value}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeName {
    String,
    Int,
    Bool,
}

impl TypeName {
    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "string" => Some(Self::String),
            "int" => Some(Self::Int),
            "bool" => Some(Self::Bool),
            _ => None,
        }
    }
}

impl fmt::Display for TypeName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => write!(formatter, "string"),
            Self::Int => write!(formatter, "int"),
            Self::Bool => write!(formatter, "bool"),
        }
    }
}
