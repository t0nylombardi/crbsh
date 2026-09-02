use std::fmt;

use crate::runtime::TypeName;

/// A static type error found without evaluating the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDiagnostic {
    pub kind: TypeDiagnosticKind,
    pub expected: Option<TypeName>,
    pub found: Option<TypeName>,
}

impl TypeDiagnostic {
    pub(crate) fn new(kind: TypeDiagnosticKind) -> Self {
        Self {
            kind,
            expected: None,
            found: None,
        }
    }

    pub(crate) fn mismatch(kind: TypeDiagnosticKind, expected: TypeName, found: TypeName) -> Self {
        Self {
            kind,
            expected: Some(expected),
            found: Some(found),
        }
    }
}

/// Identifies the language construct that failed static checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDiagnosticKind {
    AlreadyDefined(String),
    UndefinedVariable(String),
    UnknownFunction(String),
    NonValueFunction(String),
    ArgumentCount {
        function: String,
        expected: usize,
        found: usize,
    },
    MissingReturnValue(String),
    UnexpectedReturnValue,
    Declaration(String),
    Assignment(String),
    Condition,
    RangeBound,
    Iterable,
    ListElement,
    Index,
    IndexTarget,
    LengthTarget,
    MatchPattern,
    MatchArm,
    FunctionArgument {
        function: String,
        index: usize,
    },
    BinaryOperands(String),
}

impl fmt::Display for TypeDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(expected), Some(found)) = (&self.expected, &self.found) {
            return write!(
                formatter,
                "type mismatch: expected {expected}, found {found}"
            );
        }

        match &self.kind {
            TypeDiagnosticKind::AlreadyDefined(name) => {
                write!(formatter, "'{name}' is already defined in this scope")
            }
            TypeDiagnosticKind::UndefinedVariable(name) => {
                write!(formatter, "undefined variable '{name}'")
            }
            TypeDiagnosticKind::UnknownFunction(name) => {
                write!(formatter, "undefined function '{name}'")
            }
            TypeDiagnosticKind::NonValueFunction(name) => {
                write!(formatter, "function '{name}' does not return a value")
            }
            TypeDiagnosticKind::ArgumentCount {
                function,
                expected,
                found,
            } => write!(
                formatter,
                "function '{function}' expected {expected} arguments, found {found}"
            ),
            TypeDiagnosticKind::MissingReturnValue(function) => {
                write!(formatter, "function '{function}' must return a value")
            }
            TypeDiagnosticKind::UnexpectedReturnValue => {
                write!(formatter, "procedure cannot return a value")
            }
            kind => write!(formatter, "type error in {kind:?}"),
        }
    }
}
