use std::fmt;

use crate::parser::SourceLocation;
use crate::runtime::TypeName;

/// A static type error found without evaluating the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDiagnostic {
    pub kind: TypeDiagnosticKind,
    pub expected: Option<TypeName>,
    pub found: Option<TypeName>,
    location: Option<Box<SourceLocation>>,
}

impl TypeDiagnostic {
    pub(crate) fn new(kind: TypeDiagnosticKind) -> Self {
        Self {
            kind,
            expected: None,
            found: None,
            location: None,
        }
    }

    pub(crate) fn mismatch(kind: TypeDiagnosticKind, expected: TypeName, found: TypeName) -> Self {
        Self {
            kind,
            expected: Some(expected),
            found: Some(found),
            location: None,
        }
    }

    pub(crate) fn locate(&mut self, location: SourceLocation) {
        self.location.get_or_insert_with(|| Box::new(location));
    }

    /// Returns the top-level source location associated with this diagnostic.
    pub fn location(&self) -> Option<SourceLocation> {
        self.location.as_deref().copied()
    }
}

/// Identifies the language construct that failed static checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDiagnosticKind {
    AlreadyDefined(String),
    UndefinedVariable(String),
    UnknownFunction(String),
    UnknownType(String),
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
    FieldTarget,
    MissingRecordField(String),
    MissingConstructionField(String),
    UnexpectedConstructionField(String),
    LengthTarget,
    MatchPattern,
    MatchArm,
    FunctionArgument {
        function: String,
        index: usize,
    },
    StageArgumentCount {
        stage: String,
        expected: usize,
        found: usize,
    },
    StageArgument {
        stage: String,
        index: usize,
    },
    MissingStageInput(String),
    UnexpectedStageInput(String),
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
            TypeDiagnosticKind::UnknownType(name) => write!(formatter, "undefined type '{name}'"),
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
            TypeDiagnosticKind::MissingRecordField(name) => {
                write!(formatter, "record has no field '{name}'")
            }
            TypeDiagnosticKind::MissingConstructionField(name) => {
                write!(formatter, "construction is missing field '{name}'")
            }
            TypeDiagnosticKind::UnexpectedConstructionField(name) => {
                write!(formatter, "construction has unexpected field '{name}'")
            }
            TypeDiagnosticKind::StageArgumentCount {
                stage,
                expected,
                found,
            } => write!(
                formatter,
                "stage '{stage}' expected {expected} arguments, found {found}"
            ),
            TypeDiagnosticKind::MissingStageInput(stage) => {
                write!(formatter, "stage '{stage}' requires structured input")
            }
            TypeDiagnosticKind::UnexpectedStageInput(stage) => {
                write!(formatter, "stage '{stage}' cannot consume structured input")
            }
            kind => write!(formatter, "type error in {kind:?}"),
        }
    }
}
