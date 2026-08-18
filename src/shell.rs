use std::collections::HashMap;
use std::fmt;

use crate::builtins::registry::BuiltinRegistry;
use crate::parser::Expression;
use crate::value::{TypeName, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellError {
    UndefinedVariable(String),
    UndefinedEnvironmentVariable(String),
    VariableAlreadyDefined(String),
    VariableNotDefined(String),
    TypeMismatch { expected: TypeName, found: TypeName },
}

pub struct Shell {
    pub builtins: BuiltinRegistry,
    pub exit_code: i32,
    variables: HashMap<String, Value>,
    environment: HashMap<String, String>,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            exit_code: 0,
            variables: HashMap::new(),
            environment: HashMap::new(),
        }
    }

    pub fn declare_variable(
        &mut self,
        name: impl Into<String>,
        type_annotation: Option<TypeName>,
        value: Value,
    ) -> Result<(), ShellError> {
        let name = name.into();

        if self.variables.contains_key(&name) {
            return Err(ShellError::VariableAlreadyDefined(name));
        }

        enforce_type(type_annotation, &value)?;

        self.variables.insert(name, value);

        Ok(())
    }

    pub fn assign_variable(
        &mut self,
        name: impl Into<String>,
        value: Value,
    ) -> Result<(), ShellError> {
        let name = name.into();
        let Some(existing) = self.variables.get_mut(&name) else {
            return Err(ShellError::VariableNotDefined(name));
        };

        let expected = existing.type_name();
        let found = value.type_name();

        if expected != found {
            return Err(ShellError::TypeMismatch { expected, found });
        }

        *existing = value;

        Ok(())
    }

    #[cfg(test)]
    pub fn set_variable(&mut self, name: impl Into<String>, value: Value) {
        self.variables.insert(name.into(), value);
    }

    pub fn set_environment(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.environment.insert(name.into(), value.into());
    }

    pub fn environment_overrides(&self) -> impl Iterator<Item = (&str, &str)> {
        self.environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn evaluate(&self, expression: &Expression) -> Result<Value, ShellError> {
        match expression {
            Expression::Literal(value) => Ok(value.clone()),
            Expression::Identifier(name) => self
                .variables
                .get(name)
                .cloned()
                .ok_or_else(|| ShellError::UndefinedVariable(name.clone())),
            Expression::EnvironmentVariable(name) => self
                .environment_value(name)
                .map(Value::String)
                .ok_or_else(|| ShellError::UndefinedEnvironmentVariable(name.clone())),
            Expression::Status => Ok(Value::Int(i64::from(self.exit_code))),
        }
    }

    pub fn resolve_argument(&self, expression: &Expression) -> Result<String, ShellError> {
        match expression {
            Expression::Identifier(name) => Ok(self
                .variables
                .get(name)
                .map(ToString::to_string)
                .unwrap_or_else(|| name.clone())),
            _ => self.evaluate(expression).map(|value| value.to_string()),
        }
    }

    // Reserved for future line-editor completion; `env.` is a namespace prefix,
    // not an executable command.
    #[allow(dead_code)]
    pub fn complete_environment_names(&self, prefix: &str) -> Vec<String> {
        let mut names = std::env::vars()
            .map(|(name, _)| name)
            .chain(self.environment.keys().cloned())
            .filter(|name| name.starts_with(prefix))
            .collect::<Vec<_>>();

        names.sort();
        names.dedup();
        names
    }

    fn environment_value(&self, name: &str) -> Option<String> {
        self.environment
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedVariable(name) => write!(formatter, "undefined variable '{name}'"),
            Self::UndefinedEnvironmentVariable(name) => {
                write!(formatter, "undefined environment variable '{name}'")
            }
            Self::VariableAlreadyDefined(name) => {
                write!(formatter, "variable '{name}' already defined")
            }
            Self::VariableNotDefined(name) => write!(formatter, "variable '{name}' is not defined"),
            Self::TypeMismatch { expected, found } => {
                write!(
                    formatter,
                    "type mismatch: expected {expected}, found {found}"
                )
            }
        }
    }
}

fn enforce_type(type_annotation: Option<TypeName>, value: &Value) -> Result<(), ShellError> {
    let Some(expected) = type_annotation else {
        return Ok(());
    };

    let found = value.type_name();

    if expected == found {
        Ok(())
    } else {
        Err(ShellError::TypeMismatch { expected, found })
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::Expression;
    use crate::value::{TypeName, Value};

    use super::{Shell, ShellError};

    #[test]
    fn resolves_native_variables() {
        let mut shell = Shell::new();

        shell.set_variable("project", Value::String("crbsh".into()));
        shell.set_variable("retries", Value::Int(3));

        assert_eq!(
            shell.evaluate(&Expression::Identifier("project".into())),
            Ok(Value::String("crbsh".into()))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("retries".into())),
            Ok(Value::Int(3))
        );
    }

    #[test]
    fn resolves_status() {
        let mut shell = Shell::new();

        shell.exit_code = 127;

        assert_eq!(shell.evaluate(&Expression::Status), Ok(Value::Int(127)));
    }

    #[test]
    fn resolves_environment_overrides() {
        let mut shell = Shell::new();

        shell.set_environment("CRBSH_TEST_ENV", "debug");

        assert_eq!(
            shell.evaluate(&Expression::EnvironmentVariable("CRBSH_TEST_ENV".into())),
            Ok(Value::String("debug".into()))
        );
    }

    #[test]
    fn leaves_unknown_identifier_as_literal_argument() {
        let shell = Shell::new();

        assert_eq!(
            shell.resolve_argument(&Expression::Identifier("crab".into())),
            Ok("crab".into())
        );
    }

    #[test]
    fn reports_undefined_variable_during_strict_evaluation() {
        let shell = Shell::new();

        assert_eq!(
            shell.evaluate(&Expression::Identifier("missing".into())),
            Err(ShellError::UndefinedVariable("missing".into()))
        );
    }

    #[test]
    fn reports_undefined_environment_variable() {
        let mut shell = Shell::new();

        shell.set_environment("CRBSH_PRESENT", "ok");

        assert_eq!(
            shell.evaluate(&Expression::EnvironmentVariable(
                "CRBSH_DEFINITELY_MISSING".into()
            )),
            Err(ShellError::UndefinedEnvironmentVariable(
                "CRBSH_DEFINITELY_MISSING".into()
            ))
        );
    }

    #[test]
    fn declares_typed_variables() {
        let mut shell = Shell::new();

        assert_eq!(
            shell.declare_variable("enabled", Some(TypeName::Bool), Value::Bool(true)),
            Ok(())
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("enabled".into())),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn rejects_type_annotation_mismatch() {
        let mut shell = Shell::new();

        assert_eq!(
            shell.declare_variable(
                "retries",
                Some(TypeName::Int),
                Value::String("three".into())
            ),
            Err(ShellError::TypeMismatch {
                expected: TypeName::Int,
                found: TypeName::String,
            })
        );
    }

    #[test]
    fn rejects_redeclaration() {
        let mut shell = Shell::new();

        shell
            .declare_variable("project", None, Value::String("crbsh".into()))
            .unwrap();

        assert_eq!(
            shell.declare_variable("project", None, Value::String("other".into())),
            Err(ShellError::VariableAlreadyDefined("project".into()))
        );
    }

    #[test]
    fn reassigns_existing_variable_with_same_type() {
        let mut shell = Shell::new();

        shell
            .declare_variable("retries", None, Value::Int(3))
            .unwrap();
        shell.assign_variable("retries", Value::Int(5)).unwrap();

        assert_eq!(
            shell.evaluate(&Expression::Identifier("retries".into())),
            Ok(Value::Int(5))
        );
    }

    #[test]
    fn rejects_reassignment_type_change() {
        let mut shell = Shell::new();

        shell
            .declare_variable("retries", None, Value::Int(3))
            .unwrap();

        assert_eq!(
            shell.assign_variable("retries", Value::String("five".into())),
            Err(ShellError::TypeMismatch {
                expected: TypeName::Int,
                found: TypeName::String,
            })
        );
    }

    #[test]
    fn rejects_assignment_to_unknown_variable() {
        let mut shell = Shell::new();

        assert_eq!(
            shell.assign_variable("retries", Value::Int(5)),
            Err(ShellError::VariableNotDefined("retries".into()))
        );
    }
}
