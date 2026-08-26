use std::collections::HashMap;
use std::fmt;

use crate::builtins::registry::BuiltinRegistry;
use crate::history::History;
use crate::jobs::JobManager;
use crate::parser::{BinaryOperator, Expression, FunctionDefinition, ParsedCommand};
use crate::value::{TypeName, Value};

pub const MAX_FUNCTION_CALL_DEPTH: usize = 100;

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
}

pub struct Shell {
    pub builtins: BuiltinRegistry,
    pub aliases: HashMap<String, String>,
    pub history: History,
    pub jobs: JobManager,
    pub exit_code: i32,
    scopes: Vec<HashMap<String, Value>>,
    functions: HashMap<String, FunctionDefinition>,
    environment: HashMap<String, String>,
    function_call_depth: usize,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            builtins: BuiltinRegistry::new(),
            aliases: HashMap::new(),
            history: History::default(),
            jobs: JobManager::new(),
            exit_code: 0,
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            environment: HashMap::new(),
            function_call_depth: 0,
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
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Replaces the caller's local scopes with a fresh function scope while
    /// keeping the global scope visible. The returned scopes must be restored
    /// after the function finishes.
    pub fn enter_function_scope(&mut self) -> Vec<HashMap<String, Value>> {
        let global_scope = self.scopes.first().cloned().unwrap_or_default();

        std::mem::replace(&mut self.scopes, vec![global_scope, HashMap::new()])
    }

    pub fn restore_caller_scopes(&mut self, scopes: Vec<HashMap<String, Value>>) {
        self.scopes = scopes;
    }

    pub fn declare_variable(
        &mut self,
        name: impl Into<String>,
        type_annotation: Option<TypeName>,
        value: Value,
    ) -> Result<(), ShellError> {
        let name = name.into();

        let scope = self.current_scope_mut();

        if scope.contains_key(&name) {
            return Err(ShellError::VariableAlreadyDefined(name));
        }

        enforce_type(type_annotation, &value)?;

        scope.insert(name, value);

        Ok(())
    }

    pub fn assign_variable(
        &mut self,
        name: impl Into<String>,
        value: Value,
    ) -> Result<(), ShellError> {
        let name = name.into();
        let Some(existing) = self.find_variable_mut(&name) else {
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
        self.current_scope_mut().insert(name.into(), value);
    }

    pub fn define_function(&mut self, name: impl Into<String>, definition: FunctionDefinition) {
        self.functions.insert(name.into(), definition);
    }

    pub fn function(&self, name: &str) -> Option<FunctionDefinition> {
        self.functions.get(name).cloned()
    }

    pub fn enter_function_call(&mut self) -> Result<(), usize> {
        if self.function_call_depth >= MAX_FUNCTION_CALL_DEPTH {
            return Err(MAX_FUNCTION_CALL_DEPTH);
        }

        self.function_call_depth += 1;
        Ok(())
    }

    pub fn exit_function_call(&mut self) {
        debug_assert!(self.function_call_depth > 0);
        self.function_call_depth = self.function_call_depth.saturating_sub(1);
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
        let mut variables = HashMap::new();

        for scope in &self.scopes {
            for (name, value) in scope {
                variables.insert(name.clone(), value.clone());
            }
        }

        let mut variables = variables.into_iter().collect::<Vec<_>>();
        variables.sort_by(|(left, _), (right, _)| left.cmp(right));
        variables
    }

    pub fn variable_value(&self, name: &str) -> Option<Value> {
        self.find_variable(name)
    }

    pub fn export_variable(&mut self, name: &str) -> Result<(), ShellError> {
        let Some(value) = self.find_variable(name) else {
            return Err(ShellError::VariableNotDefined(name.into()));
        };

        self.set_environment(name, value.to_string());

        Ok(())
    }

    pub fn unset_variable(&mut self, name: &str) -> bool {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.remove(name))
            .is_some()
    }

    pub fn evaluate(&self, expression: &Expression) -> Result<Value, ShellError> {
        match expression {
            Expression::Literal(value) => Ok(value.clone()),
            Expression::Identifier(name) => self
                .find_variable(name)
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
            Expression::Call { name, .. } => Err(ShellError::UnsupportedCall(name.clone())),
        }
    }

    pub fn resolve_argument(&self, expression: &Expression) -> Result<String, ShellError> {
        match expression {
            Expression::Identifier(name) => Ok(self
                .find_variable(name)
                .map(|value| value.to_string())
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

    pub fn environment_value(&self, name: &str) -> Option<String> {
        self.environment
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }

    fn current_scope_mut(&mut self) -> &mut HashMap<String, Value> {
        self.scopes
            .last_mut()
            .expect("shell always has at least one variable scope")
    }

    fn find_variable(&self, name: &str) -> Option<Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn find_variable_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
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
    use crate::parser::{BinaryOperator, Expression};
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
