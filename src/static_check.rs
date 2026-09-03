use std::fmt;

use crab_lang::parser::{LocatedInput, SourceDiagnostic, SourceLocation, parse_source};
use crab_lang::type_checker::{TypeChecker, TypeDiagnostic};

use crate::execution::SHELL_HOST_TYPES;

/// A syntax or type diagnostic found before script execution.
#[derive(Debug)]
pub(crate) enum StaticDiagnostic {
    Syntax(SourceDiagnostic),
    Type(TypeDiagnostic),
    UnsupportedDirective {
        location: SourceLocation,
        name: &'static str,
    },
}

impl StaticDiagnostic {
    pub(crate) fn location(&self) -> Option<SourceLocation> {
        match self {
            Self::Syntax(diagnostic) => Some(diagnostic.location),
            Self::Type(diagnostic) => diagnostic.location(),
            Self::UnsupportedDirective { location, .. } => Some(*location),
        }
    }
}

impl fmt::Display for StaticDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(diagnostic) => write!(formatter, "{}", diagnostic.error),
            Self::Type(diagnostic) => write!(formatter, "{diagnostic}"),
            Self::UnsupportedDirective { name, .. } => {
                write!(formatter, "'{name}' is only supported in .crb scripts")
            }
        }
    }
}

/// Parses and type checks a complete script without evaluating it.
pub(crate) fn check_source(source: &str) -> Result<Vec<LocatedInput>, Vec<StaticDiagnostic>> {
    let program = parse_source(source).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(StaticDiagnostic::Syntax)
            .collect::<Vec<_>>()
    })?;

    let directives = program
        .iter()
        .filter_map(|located| {
            let name = match located.input {
                crab_lang::parser::ParsedInput::Module { .. } => "module",
                crab_lang::parser::ParsedInput::Import { .. } => "import",
                _ => return None,
            };
            Some(StaticDiagnostic::UnsupportedDirective {
                location: located.location,
                name,
            })
        })
        .collect::<Vec<_>>();
    if !directives.is_empty() {
        return Err(directives);
    }

    check_program(&program)?;

    Ok(program)
}

/// Type checks an already parsed multi-file program.
pub(crate) fn check_program(program: &[LocatedInput]) -> Result<(), Vec<StaticDiagnostic>> {
    TypeChecker::check_located_with_host(program, &SHELL_HOST_TYPES).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(StaticDiagnostic::Type)
            .collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use crab_lang::type_checker::TypeDiagnosticKind;

    use super::*;

    #[test]
    fn aggregates_type_diagnostics_with_source_locations() {
        let diagnostics =
            check_source("let count: int = \"many\"\nlet ready: bool = 1\n").unwrap_err();

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].location().unwrap().line, 1);
        assert_eq!(diagnostics[1].location().unwrap().line, 2);
        assert!(matches!(
            diagnostics[0],
            StaticDiagnostic::Type(TypeDiagnostic {
                kind: TypeDiagnosticKind::Declaration(_),
                ..
            })
        ));
    }

    #[test]
    fn recognizes_shell_owned_structured_stages() {
        assert!(check_source("values [1, 2] | count\n").is_ok());
    }
}
