use std::collections::HashMap;
use std::fmt;

use crate::builtins::registry::BuiltinRegistry;
use crate::execution::JobManager;
use crate::history::History;
use crate::parser::{BinaryOperator, Expression, FunctionDefinition, MatchPattern, ParsedCommand};
use crate::runtime::{LanguageRuntime, ScopeError, ScopeStack, TypeName, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasError {
    InvalidReplacement(String),
    Cycle(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellError {
    DivisionByZero,
    IntegerOverflow,
    UndefinedVariable(String),
    UndefinedEnvironmentVariable(String),
    UnsupportedCall(String),
    UnsupportedOperator {
        operator: BinaryOperator,
        left: TypeName,
        right: TypeName,
    },
    VariableAlreadyDefined(String),
    VariableNotDefined(String),
    TypeMismatch {
        expected: TypeName,
        found: TypeName,
    },
    HeterogeneousList {
        expected: TypeName,
        found: TypeName,
    },
    IndexOutOfBounds {
        index: i64,
        len: usize,
    },
    NegativeIndex(i64),
    MissingRecordField(String),
    NonExhaustiveMatch,
}

pub struct Shell {
    pub builtins: BuiltinRegistry,
    pub aliases: HashMap<String, String>,
    pub history: History,
    pub jobs: JobManager,
    pub exit_code: i32,
    runtime: LanguageRuntime,
    environment: HashMap<String, String>,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            aliases: HashMap::new(),
            history: History::default(),
            jobs: JobManager::new(),
            exit_code: 0,
            runtime: LanguageRuntime::new(),
            environment: HashMap::new(),
        }
    }

    pub fn set_alias(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), AliasError> {
        let name = name.into();
        let value = value.into();

        validate_alias_name(&name)?;
        parse_alias_replacement(&value)?;

        self.aliases.insert(name, value);

        Ok(())
    }

    pub fn unset_alias(&mut self, name: &str) -> bool {
        self.aliases.remove(name).is_some()
    }

    pub fn alias_value(&self, name: &str) -> Option<String> {
        self.aliases.get(name).cloned()
    }

    pub fn aliases(&self) -> Vec<(String, String)> {
        let mut aliases = self
            .aliases
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<Vec<_>>();

        aliases.sort_by(|(left, _), (right, _)| left.cmp(right));
        aliases
    }

    pub fn alias_command(&self, name: &str) -> Result<Option<ParsedCommand>, AliasError> {
        self.aliases
            .get(name)
            .map(|value| parse_alias_replacement(value))
            .transpose()
    }

    /// Adds a lexical block scope. Variable lookup walks these frames from the
    /// innermost block toward the outermost visible scope.
    pub fn push_scope(&mut self) {
        self.runtime.push_scope();
    }

    pub fn pop_scope(&mut self) {
        self.runtime.pop_scope();
    }

    /// Replaces the caller's local scopes with a fresh function scope while
    /// keeping the global scope visible. The returned scopes must be restored
    /// after the function finishes.
    pub(crate) fn enter_function_scope(&mut self) -> ScopeStack {
        self.runtime.enter_function_scope()
    }

    pub(crate) fn restore_caller_scopes(&mut self, scopes: ScopeStack) {
        self.runtime.restore_caller_scopes(scopes);
    }

    pub fn declare_variable(
        &mut self,
        name: impl Into<String>,
        type_annotation: Option<TypeName>,
        value: Value,
    ) -> Result<(), ShellError> {
        self.runtime
            .declare_variable(name.into(), type_annotation, value)
            .map_err(ShellError::from)
    }

    pub fn assign_variable(
        &mut self,
        name: impl Into<String>,
        value: Value,
    ) -> Result<(), ShellError> {
        self.runtime
            .assign_variable(name.into(), value)
            .map_err(ShellError::from)
    }

    #[cfg(test)]
    pub fn set_variable(&mut self, name: impl Into<String>, value: Value) {
        self.runtime.set_variable(name.into(), value);
    }

    pub fn define_function(&mut self, name: impl Into<String>, definition: FunctionDefinition) {
        self.runtime.define_function(name.into(), definition);
    }

    pub fn function(&self, name: &str) -> Option<FunctionDefinition> {
        self.runtime.function(name)
    }

    pub fn enter_function_call(&mut self) -> Result<(), usize> {
        self.runtime.enter_function_call()
    }

    pub fn exit_function_call(&mut self) {
        self.runtime.exit_function_call();
    }

    pub fn set_environment(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.environment.insert(name.into(), value.into());
    }

    pub fn unset_environment(&mut self, name: &str) -> bool {
        self.environment.remove(name).is_some()
    }

    pub fn environment_overrides(&self) -> impl Iterator<Item = (&str, &str)> {
        self.environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn variables(&self) -> Vec<(String, Value)> {
        self.runtime.variables()
    }

    pub fn variable_value(&self, name: &str) -> Option<Value> {
        self.runtime.variable_value(name)
    }

    pub fn export_variable(&mut self, name: &str) -> Result<(), ShellError> {
        let Some(value) = self.runtime.variable_value(name) else {
            return Err(ShellError::VariableNotDefined(name.into()));
        };

        self.set_environment(name, value.to_string());

        Ok(())
    }

    pub fn unset_variable(&mut self, name: &str) -> bool {
        self.runtime.remove_variable(name)
    }

    pub fn evaluate(&self, expression: &Expression) -> Result<Value, ShellError> {
        match expression {
            Expression::Literal(value) => Ok(value.clone()),
            Expression::Identifier(name) => self
                .runtime
                .variable_value(name)
                .ok_or_else(|| ShellError::UndefinedVariable(name.clone())),
            Expression::EnvironmentVariable(name) => self
                .environment_value(name)
                .map(Value::String)
                .ok_or_else(|| ShellError::UndefinedEnvironmentVariable(name.clone())),
            Expression::Status => Ok(Value::Int(i64::from(self.exit_code))),
            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left)?;
                let right = self.evaluate(right)?;

                evaluate_binary(*operator, left, right)
            }
            Expression::List(values) => values
                .iter()
                .map(|value| self.evaluate(value))
                .collect::<Result<Vec<_>, _>>()
                .and_then(validate_list_values)
                .map(Value::List),
            Expression::Index { target, index } => {
                evaluate_index(self.evaluate(target)?, self.evaluate(index)?)
            }
            Expression::Field { target, name } => evaluate_field(self.evaluate(target)?, name),
            Expression::Match { value, arms } => {
                let value = self.evaluate(value)?;
                let arm = arms
                    .iter()
                    .find(|arm| match &arm.pattern {
                        MatchPattern::Wildcard => true,
                        MatchPattern::Literal(pattern) => pattern == &value,
                        MatchPattern::Status => value == Value::Int(i64::from(self.exit_code)),
                        MatchPattern::Identifier(name) => self
                            .runtime
                            .variable_value(name)
                            .is_some_and(|pattern| pattern == value),
                    })
                    .ok_or(ShellError::NonExhaustiveMatch)?;
                self.evaluate(&arm.value)
            }
            Expression::Len(target) => evaluate_len(self.evaluate(target)?),
            Expression::Call { name, .. } => Err(ShellError::UnsupportedCall(name.clone())),
        }
    }

    pub fn resolve_argument(&self, expression: &Expression) -> Result<String, ShellError> {
        match expression {
            Expression::Identifier(name) => Ok(self
                .runtime
                .variable_value(name)
                .map(|value| value.to_string())
                .unwrap_or_else(|| name.clone())),
            Expression::Field { .. } => {
                let Some((root, path)) = field_path(expression) else {
                    return self.evaluate(expression).map(|value| value.to_string());
                };
                if self.runtime.variable_value(root).is_none() {
                    return Ok(path);
                }
                self.evaluate(expression).map(|value| value.to_string())
            }
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

    pub fn environment_value(&self, name: &str) -> Option<String> {
        self.environment
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }
}

fn field_path(expression: &Expression) -> Option<(&str, String)> {
    match expression {
        Expression::Identifier(name) => Some((name, name.clone())),
        Expression::Field { target, name } => {
            let (root, target) = field_path(target)?;
            Some((root, format!("{target}.{name}")))
        }
        _ => None,
    }
}

impl From<ScopeError> for ShellError {
    fn from(error: ScopeError) -> Self {
        match error {
            ScopeError::AlreadyDefined(name) => Self::VariableAlreadyDefined(name),
            ScopeError::NotDefined(name) => Self::VariableNotDefined(name),
            ScopeError::TypeMismatch { expected, found } => Self::TypeMismatch { expected, found },
        }
    }
}

impl fmt::Display for AliasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReplacement(message) => write!(formatter, "{message}"),
            Self::Cycle(names) => write!(
                formatter,
                "alias expansion cycle detected: {}",
                names.join(" -> ")
            ),
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DivisionByZero => write!(formatter, "division by zero"),
            Self::IntegerOverflow => write!(formatter, "integer overflow"),
            Self::UndefinedVariable(name) => write!(formatter, "undefined variable '{name}'"),
            Self::UndefinedEnvironmentVariable(name) => {
                write!(formatter, "undefined environment variable '{name}'")
            }
            Self::UnsupportedCall(name) => {
                write!(
                    formatter,
                    "function call '{name}' is not supported in this evaluation context"
                )
            }
            Self::UnsupportedOperator {
                operator,
                left,
                right,
            } => write!(
                formatter,
                "unsupported operator {} for {left} and {right}",
                operator.symbol()
            ),
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
            Self::HeterogeneousList { expected, found } => write!(
                formatter,
                "list elements must have one type: expected {expected}, found {found}"
            ),
            Self::IndexOutOfBounds { index, len } => {
                write!(
                    formatter,
                    "list index {index} out of bounds for length {len}"
                )
            }
            Self::NegativeIndex(index) => {
                write!(formatter, "list index cannot be negative: {index}")
            }
            Self::MissingRecordField(name) => write!(formatter, "record has no field '{name}'"),
            Self::NonExhaustiveMatch => write!(formatter, "non-exhaustive match expression"),
        }
    }
}

fn validate_alias_name(name: &str) -> Result<(), AliasError> {
    if name.is_empty() {
        return Err(AliasError::InvalidReplacement(
            "alias name cannot be empty".into(),
        ));
    }

    if name.contains(['=', '|', '&', '<', '>', ' ', '\t', '\n']) {
        return Err(AliasError::InvalidReplacement(format!(
            "invalid alias name '{name}'"
        )));
    }

    Ok(())
}

fn parse_alias_replacement(value: &str) -> Result<ParsedCommand, AliasError> {
    let parsed = crate::parser::parse(value).map_err(|err| {
        AliasError::InvalidReplacement(format!(
            "invalid alias replacement: {}",
            crate::parser::format_error(&err)
        ))
    })?;

    let crate::parser::ParsedInput::Pipeline(pipeline) = parsed else {
        return Err(AliasError::InvalidReplacement(
            "alias replacement must be a single command without redirection".into(),
        ));
    };

    let [command] = pipeline.commands.as_slice() else {
        return Err(AliasError::InvalidReplacement(
            "alias replacement must be a single command without redirection".into(),
        ));
    };

    if !command.redirections.is_empty() {
        return Err(AliasError::InvalidReplacement(
            "alias replacement must be a single command without redirection".into(),
        ));
    }

    Ok(command.clone())
}

pub fn evaluate_binary(
    operator: BinaryOperator,
    left: Value,
    right: Value,
) -> Result<Value, ShellError> {
    match (operator, left, right) {
        (BinaryOperator::Add, Value::Int(left), Value::Int(right)) => left
            .checked_add(right)
            .map(Value::Int)
            .ok_or(ShellError::IntegerOverflow),
        (BinaryOperator::Subtract, Value::Int(left), Value::Int(right)) => left
            .checked_sub(right)
            .map(Value::Int)
            .ok_or(ShellError::IntegerOverflow),
        (BinaryOperator::Multiply, Value::Int(left), Value::Int(right)) => left
            .checked_mul(right)
            .map(Value::Int)
            .ok_or(ShellError::IntegerOverflow),
        (BinaryOperator::Divide, Value::Int(_), Value::Int(0)) => Err(ShellError::DivisionByZero),
        (BinaryOperator::Divide, Value::Int(left), Value::Int(right)) => left
            .checked_div(right)
            .map(Value::Int)
            .ok_or(ShellError::IntegerOverflow),
        (BinaryOperator::Equal, left, right) if left.type_name() == right.type_name() => {
            Ok(Value::Bool(left == right))
        }
        (BinaryOperator::NotEqual, left, right) if left.type_name() == right.type_name() => {
            Ok(Value::Bool(left != right))
        }
        (BinaryOperator::Less, Value::Int(left), Value::Int(right)) => {
            Ok(Value::Bool(left < right))
        }
        (BinaryOperator::LessEqual, Value::Int(left), Value::Int(right)) => {
            Ok(Value::Bool(left <= right))
        }
        (BinaryOperator::Greater, Value::Int(left), Value::Int(right)) => {
            Ok(Value::Bool(left > right))
        }
        (BinaryOperator::GreaterEqual, Value::Int(left), Value::Int(right)) => {
            Ok(Value::Bool(left >= right))
        }
        (operator, left, right) => Err(ShellError::UnsupportedOperator {
            operator,
            left: left.type_name(),
            right: right.type_name(),
        }),
    }
}

pub fn validate_list_values(values: Vec<Value>) -> Result<Vec<Value>, ShellError> {
    let Some(expected) = values.first().map(Value::type_name) else {
        return Ok(values);
    };

    if let Some(found) = values
        .iter()
        .skip(1)
        .map(Value::type_name)
        .find(|found| !expected.accepts(found))
    {
        return Err(ShellError::HeterogeneousList { expected, found });
    }

    Ok(values)
}

pub fn evaluate_index(target: Value, index: Value) -> Result<Value, ShellError> {
    let Value::List(values) = target else {
        return Err(ShellError::TypeMismatch {
            expected: TypeName::List(None),
            found: target.type_name(),
        });
    };
    let Value::Int(index) = index else {
        return Err(ShellError::TypeMismatch {
            expected: TypeName::Int,
            found: index.type_name(),
        });
    };
    let index = usize::try_from(index).map_err(|_| ShellError::NegativeIndex(index))?;

    values
        .get(index)
        .cloned()
        .ok_or(ShellError::IndexOutOfBounds {
            index: index as i64,
            len: values.len(),
        })
}

pub fn evaluate_field(target: Value, name: &str) -> Result<Value, ShellError> {
    let Value::Record(mut fields) = target else {
        return Err(ShellError::TypeMismatch {
            expected: TypeName::Record(None),
            found: target.type_name(),
        });
    };
    fields
        .remove(name)
        .ok_or_else(|| ShellError::MissingRecordField(name.into()))
}

pub fn evaluate_len(target: Value) -> Result<Value, ShellError> {
    let Value::List(values) = target else {
        return Err(ShellError::TypeMismatch {
            expected: TypeName::List(None),
            found: target.type_name(),
        });
    };
    let len = i64::try_from(values.len()).map_err(|_| ShellError::IntegerOverflow)?;
    Ok(Value::Int(len))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::parser::{BinaryOperator, Expression};
    use crate::runtime::{TypeName, Value};

    use super::{Shell, ShellError};

    #[test]
    fn evaluates_list_index_and_len() {
        let expression = Expression::List(vec![Value::Int(10).into(), Value::Int(20).into()]);
        let mut shell = Shell::new();
        shell
            .declare_variable("numbers", None, shell.evaluate(&expression).unwrap())
            .unwrap();

        assert_eq!(
            shell.evaluate(&Expression::Index {
                target: Box::new(Expression::Identifier("numbers".into())),
                index: Box::new(Value::Int(1).into()),
            }),
            Ok(Value::Int(20))
        );
        assert_eq!(
            shell.evaluate(&Expression::Len(Box::new(Expression::Identifier(
                "numbers".into()
            )))),
            Ok(Value::Int(2))
        );
    }

    #[test]
    fn evaluates_record_fields_and_preserves_dotted_command_arguments() {
        let mut shell = Shell::new();
        shell
            .declare_variable(
                "user",
                None,
                Value::Record(BTreeMap::from([(
                    "name".into(),
                    Value::String("Tony".into()),
                )])),
            )
            .unwrap();

        assert_eq!(
            shell.evaluate(&Expression::Field {
                target: Box::new(Expression::Identifier("user".into())),
                name: "name".into(),
            }),
            Ok(Value::String("Tony".into()))
        );
        assert_eq!(
            shell.resolve_argument(&Expression::Field {
                target: Box::new(Expression::Identifier("archive".into())),
                name: "tar".into(),
            }),
            Ok("archive.tar".into())
        );
    }

    #[test]
    fn rejects_heterogeneous_lists_and_invalid_indexes() {
        assert_eq!(
            super::validate_list_values(vec![Value::Int(1), Value::String("two".into())]),
            Err(ShellError::HeterogeneousList {
                expected: TypeName::Int,
                found: TypeName::String,
            })
        );
        assert_eq!(
            super::evaluate_index(Value::List(vec![Value::Int(1)]), Value::Int(-1)),
            Err(ShellError::NegativeIndex(-1))
        );
        assert_eq!(
            super::evaluate_index(Value::List(vec![Value::Int(1)]), Value::Int(1)),
            Err(ShellError::IndexOutOfBounds { index: 1, len: 1 })
        );
        assert_eq!(
            super::evaluate_index(
                Value::List(vec![Value::Int(1)]),
                Value::String("zero".into())
            ),
            Err(ShellError::TypeMismatch {
                expected: TypeName::Int,
                found: TypeName::String,
            })
        );
        assert_eq!(
            super::evaluate_index(Value::String("crab".into()), Value::Int(0)),
            Err(ShellError::TypeMismatch {
                expected: TypeName::List(None),
                found: TypeName::String,
            })
        );
    }

    #[test]
    fn typed_empty_list_preserves_its_declared_element_type() {
        let mut shell = Shell::new();
        shell
            .declare_variable(
                "numbers",
                Some(TypeName::List(Some(Box::new(TypeName::Int)))),
                Value::List(Vec::new()),
            )
            .unwrap();

        assert_eq!(
            shell.assign_variable("numbers", Value::List(vec![Value::String("wrong".into())])),
            Err(ShellError::TypeMismatch {
                expected: TypeName::List(Some(Box::new(TypeName::Int))),
                found: TypeName::List(Some(Box::new(TypeName::String))),
            })
        );
    }

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

    #[test]
    fn inner_scopes_shadow_and_outer_scope_survives() {
        let mut shell = Shell::new();

        shell.declare_variable("x", None, Value::Int(10)).unwrap();
        shell.push_scope();
        shell.declare_variable("x", None, Value::Int(20)).unwrap();

        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(20))
        );

        shell.pop_scope();

        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(10))
        );
    }

    #[test]
    fn assignment_updates_nearest_existing_scope() {
        let mut shell = Shell::new();

        shell.declare_variable("x", None, Value::Int(10)).unwrap();
        shell.push_scope();
        shell.assign_variable("x", Value::Int(20)).unwrap();
        shell.pop_scope();

        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(20))
        );
    }

    #[test]
    fn function_and_block_scopes_resolve_inside_out() {
        let mut shell = Shell::new();

        shell
            .declare_variable("global", None, Value::Int(10))
            .unwrap();
        let caller_scopes = shell.enter_function_scope();
        shell.declare_variable("x", None, Value::Int(5)).unwrap();
        shell.declare_variable("a", None, Value::Int(1)).unwrap();
        shell.push_scope();
        shell.declare_variable("b", None, Value::Int(2)).unwrap();

        assert_eq!(
            shell.evaluate(&Expression::Identifier("global".into())),
            Ok(Value::Int(10))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(5))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("a".into())),
            Ok(Value::Int(1))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("b".into())),
            Ok(Value::Int(2))
        );

        shell.pop_scope();

        assert_eq!(
            shell.evaluate(&Expression::Identifier("a".into())),
            Ok(Value::Int(1))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("b".into())),
            Err(ShellError::UndefinedVariable("b".into()))
        );

        shell.restore_caller_scopes(caller_scopes);
    }

    #[test]
    fn evaluates_integer_arithmetic() {
        let mut shell = Shell::new();

        shell
            .declare_variable("retries", None, Value::Int(3))
            .unwrap();

        assert_eq!(
            shell.evaluate(&binary(
                Expression::Identifier("retries".into()),
                BinaryOperator::Add,
                Value::Int(2).into(),
            )),
            Ok(Value::Int(5))
        );
        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(6).into(),
                BinaryOperator::Subtract,
                Value::Int(4).into(),
            )),
            Ok(Value::Int(2))
        );
        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(3).into(),
                BinaryOperator::Multiply,
                Value::Int(4).into(),
            )),
            Ok(Value::Int(12))
        );
        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(8).into(),
                BinaryOperator::Divide,
                Value::Int(2).into(),
            )),
            Ok(Value::Int(4))
        );
    }

    #[test]
    fn evaluates_integer_comparisons() {
        let shell = Shell::new();

        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(3).into(),
                BinaryOperator::Less,
                Value::Int(5).into(),
            )),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(5).into(),
                BinaryOperator::GreaterEqual,
                Value::Int(5).into(),
            )),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn evaluates_equality_for_matching_types() {
        let shell = Shell::new();

        assert_eq!(
            shell.evaluate(&binary(
                Value::Bool(true).into(),
                BinaryOperator::Equal,
                Value::Bool(true).into(),
            )),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            shell.evaluate(&binary(
                Value::String("true".into()).into(),
                BinaryOperator::NotEqual,
                Value::String("false".into()).into(),
            )),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn rejects_expression_type_mismatch() {
        let shell = Shell::new();

        assert_eq!(
            shell.evaluate(&binary(
                Value::String("3".into()).into(),
                BinaryOperator::Add,
                Value::Int(2).into(),
            )),
            Err(ShellError::UnsupportedOperator {
                operator: BinaryOperator::Add,
                left: TypeName::String,
                right: TypeName::Int,
            })
        );
    }

    #[test]
    fn reports_unsupported_call_in_shell_evaluator() {
        let shell = Shell::new();

        assert_eq!(
            shell.evaluate(&Expression::Call {
                name: "add".into(),
                args: Vec::new(),
            }),
            Err(ShellError::UnsupportedCall("add".into()))
        );
    }

    #[test]
    fn rejects_integer_overflow() {
        let shell = Shell::new();

        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(i64::MAX).into(),
                BinaryOperator::Add,
                Value::Int(1).into(),
            )),
            Err(ShellError::IntegerOverflow)
        );
        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(i64::MIN).into(),
                BinaryOperator::Divide,
                Value::Int(-1).into(),
            )),
            Err(ShellError::IntegerOverflow)
        );
    }

    #[test]
    fn rejects_division_by_zero() {
        let shell = Shell::new();

        assert_eq!(
            shell.evaluate(&binary(
                Value::Int(1).into(),
                BinaryOperator::Divide,
                Value::Int(0).into(),
            )),
            Err(ShellError::DivisionByZero)
        );
    }

    fn binary(left: Expression, operator: BinaryOperator, right: Expression) -> Expression {
        Expression::Binary {
            left: Box::new(left),
            operator,
            right: Box::new(right),
        }
    }
}
