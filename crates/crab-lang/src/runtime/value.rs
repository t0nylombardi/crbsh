use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
}

impl Value {
    pub fn type_name(&self) -> TypeName {
        match self {
            Self::String(_) => TypeName::String,
            Self::Int(_) => TypeName::Int,
            Self::Bool(_) => TypeName::Bool,
            Self::List(values) => {
                TypeName::List(values.first().map(Value::type_name).map(Box::new))
            }
            Self::Record(_) => TypeName::Record,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => write!(formatter, "{value}"),
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::List(values) => {
                let values = values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "[{values}]")
            }
            Self::Record(fields) => {
                let fields = fields
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "{{{fields}}}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeName {
    String,
    Int,
    Bool,
    List(Option<Box<TypeName>>),
    Record,
}

impl TypeName {
    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "string" => Some(Self::String),
            "int" => Some(Self::Int),
            "bool" => Some(Self::Bool),
            "record" => Some(Self::Record),
            _ => input
                .strip_prefix("list<")
                .and_then(|inner| inner.strip_suffix('>'))
                .and_then(Self::parse)
                .map(|inner| Self::List(Some(Box::new(inner)))),
        }
    }
}

impl fmt::Display for TypeName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => write!(formatter, "string"),
            Self::Int => write!(formatter, "int"),
            Self::Bool => write!(formatter, "bool"),
            Self::List(Some(element)) => write!(formatter, "list<{element}>"),
            Self::List(None) => write!(formatter, "list<?>"),
            Self::Record => write!(formatter, "record"),
        }
    }
}

impl TypeName {
    pub fn accepts(&self, actual: &Self) -> bool {
        match (self, actual) {
            (Self::List(Some(expected)), Self::List(Some(actual))) => expected.accepts(actual),
            (Self::List(_), Self::List(None)) | (Self::List(None), Self::List(_)) => true,
            _ => self == actual,
        }
    }
}
