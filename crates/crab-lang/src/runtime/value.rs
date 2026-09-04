use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    NamedRecord {
        name: String,
        fields: BTreeMap<String, Value>,
    },
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
            Self::Record(fields) => TypeName::Record(Some(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), value.type_name()))
                    .collect(),
            )),
            Self::NamedRecord { name, .. } => TypeName::Named(name.clone()),
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
            Self::NamedRecord { name, fields } => {
                let fields = fields
                    .iter()
                    .map(|(field, value)| format!("{field}: {value}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "{name} {{{fields}}}")
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
    Record(Option<BTreeMap<String, TypeName>>),
    Named(String),
}

impl TypeName {
    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "string" => Some(Self::String),
            "int" => Some(Self::Int),
            "bool" => Some(Self::Bool),
            "record" => Some(Self::Record(None)),
            _ => input
                .strip_prefix("list<")
                .and_then(|inner| inner.strip_suffix('>'))
                .and_then(Self::parse)
                .map(|inner| Self::List(Some(Box::new(inner))))
                .or_else(|| valid_type_identifier(input).then(|| Self::Named(input.into()))),
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
            Self::Record(Some(fields)) => {
                let fields = fields
                    .iter()
                    .map(|(name, type_name)| format!("{name}: {type_name}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "record{{{fields}}}")
            }
            Self::Record(None) => write!(formatter, "record"),
            Self::Named(name) => write!(formatter, "{name}"),
        }
    }
}

impl TypeName {
    pub fn accepts(&self, actual: &Self) -> bool {
        match (self, actual) {
            (Self::List(Some(expected)), Self::List(Some(actual))) => expected.accepts(actual),
            (Self::List(_), Self::List(None)) | (Self::List(None), Self::List(_)) => true,
            (Self::Record(Some(expected)), Self::Record(Some(actual))) => {
                expected.len() == actual.len()
                    && expected.iter().all(|(name, expected_type)| {
                        actual
                            .get(name)
                            .is_some_and(|actual_type| expected_type.accepts(actual_type))
                    })
            }
            (Self::Record(_), Self::Record(None)) | (Self::Record(None), Self::Record(_)) => true,
            (Self::Named(expected), Self::Named(actual)) => expected == actual,
            _ => self == actual,
        }
    }
}

fn valid_type_identifier(input: &str) -> bool {
    let valid = input.split("::").all(|part| {
        let mut chars = part.chars();
        matches!(chars.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
            && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    });
    let name = input.rsplit("::").next().unwrap_or_default();
    valid && name.starts_with(|ch: char| ch.is_ascii_uppercase())
}
