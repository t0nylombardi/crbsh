use std::fs;
use std::path::Path;

use crate::builtins::BuiltinOutcome;
use crate::execution;
use crate::parser::{
    self, Expression, Iterable, ParsedCommand, ParsedInput, Pipeline, PipelineConnector,
};
use crate::shell::{self, Shell, ShellError};

use super::{TypeName, Value};

pub(crate) enum EvalError {
    Shell(ShellError),
    Function(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell(error) => write!(formatter, "{error}"),
            Self::Function(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<ShellError> for EvalError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

pub(crate) enum ControlFlow {
    Continue,
    Break,
    LoopContinue,
    Return(Option<Value>),
    Exit(i32),
    Error(String),
}

pub(crate) fn execute_input(shell: &mut Shell, parsed_input: ParsedInput) -> ControlFlow {
    match parsed_input {
        ParsedInput::FunctionDefinition { name, definition } => {
            shell.define_function(name, definition);
            shell.exit_code = 0;
        }

        ParsedInput::Let {
            name,
            type_annotation,
            value,
        } => {
            let value = match evaluate_expression(shell, &value) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                    return ControlFlow::Continue;
                }
            };

            match shell.declare_variable(name, type_annotation, value) {
                Ok(()) => shell.exit_code = 0,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                }
            }
        }

        ParsedInput::Break => return ControlFlow::Break,

        ParsedInput::Continue => return ControlFlow::LoopContinue,

        ParsedInput::Return { value } => {
            let value = match value {
                Some(value) => match evaluate_expression(shell, &value) {
                    Ok(value) => Some(value),
                    Err(err) => {
                        shell.exit_code = 2;
                        return ControlFlow::Error(err.to_string());
                    }
                },
                None => None,
            };

            return ControlFlow::Return(value);
        }

        ParsedInput::Assignment { name, value } => {
            let value = match evaluate_expression(shell, &value) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                    return ControlFlow::Continue;
                }
            };

            match shell.assign_variable(name, value) {
                Ok(()) => shell.exit_code = 0,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                }
            }
        }

        ParsedInput::EnvironmentAssignment { name, value } => {
            match evaluate_expression(shell, &value) {
                Ok(value) => {
                    shell.set_environment(name, value.to_string());
                    shell.exit_code = 0;
                }
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                }
            }
        }

        ParsedInput::If {
            branches,
            else_body,
        } => {
            let flow = execute_if(shell, branches, else_body);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::Match { value, arms } => {
            let flow = execute_match(shell, value, arms);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::While { condition, body } => {
            let flow = execute_while(shell, condition, body);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::For {
            name,
            iterable,
            body,
        } => {
            let flow = execute_for(shell, name, iterable, body);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }
        }

        ParsedInput::Pipeline(pipeline) => return execute_pipeline_input(shell, pipeline),

        ParsedInput::PipelineChain { first, rest } => {
            let flow = execute_pipeline_input(shell, first);

            if !matches!(flow, ControlFlow::Continue) {
                return flow;
            }

            for (connector, pipeline) in rest {
                let should_execute = match connector {
                    PipelineConnector::And => shell.exit_code == 0,
                    PipelineConnector::Or => shell.exit_code != 0,
                };

                if !should_execute {
                    continue;
                }

                let flow = execute_pipeline_input(shell, pipeline);

                if !matches!(flow, ControlFlow::Continue) {
                    return flow;
                }
            }
        }

        ParsedInput::BackgroundPipeline { pipeline, command } => {
            let pipeline = match expand_pipeline_aliases(shell, pipeline) {
                Ok(pipeline) => pipeline,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                    return ControlFlow::Continue;
                }
            };

            if pipeline.commands.is_empty() {
                shell.exit_code = 0;
                return ControlFlow::Continue;
            }

            match execution::execute_background_pipeline(shell, &pipeline, command) {
                Ok((id, pid)) => {
                    println!("[{id}] {pid}");
                    shell.exit_code = 0;
                }

                Err(err) => {
                    eprintln!("crbsh: {}: {}", err.command, err.message);
                    shell.exit_code = 127;
                }
            }
        }
    }

    ControlFlow::Continue
}

fn execute_pipeline_input(shell: &mut Shell, pipeline: Pipeline) -> ControlFlow {
    let pipeline = match expand_pipeline_aliases(shell, pipeline) {
        Ok(pipeline) => pipeline,
        Err(err) => {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }
    };

    let parsed = match pipeline.commands.first() {
        Some(command) => command,
        None => {
            shell.exit_code = 0;
            return ControlFlow::Continue;
        }
    };

    let command = &parsed.name;
    let args = &parsed.args;

    if pipeline.commands.len() == 1
        && parsed.redirections.is_empty()
        && shell.function(command).is_some()
    {
        let result = execute_function_call(shell, command, args);

        match result {
            Ok(_) => shell.exit_code = 0,
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
            }
        }

        return ControlFlow::Continue;
    }

    if pipeline.commands.len() == 1
        && parsed.redirections.is_empty()
        && let Some(builtin) = shell.builtins.get(command)
    {
        let resolved_args = if uses_raw_builtin_args(command) {
            args.iter().map(raw_builtin_arg).collect()
        } else {
            args.iter()
                .map(|arg| shell.resolve_argument(arg))
                .collect::<Result<Vec<_>, _>>()
        };

        let resolved_args = match resolved_args {
            Ok(args) => args,
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 1;
                return ControlFlow::Continue;
            }
        };

        match builtin(shell, &resolved_args) {
            Ok(BuiltinOutcome::Continue) => {
                shell.exit_code = 0;
            }

            Ok(BuiltinOutcome::ContinueWithStatus(code)) => {
                shell.exit_code = code;
            }

            Ok(BuiltinOutcome::Exit(code)) => {
                return ControlFlow::Exit(code);
            }

            Err(err) => {
                eprintln!("crbsh: {}", err.message);
                shell.exit_code = 1;
            }
        }

        return ControlFlow::Continue;
    }

    match execution::execute_pipeline(shell, &pipeline) {
        Ok(code) => {
            shell.exit_code = code;
        }

        Err(err) => {
            eprintln!("crbsh: {}: {}", err.command, err.message);
            shell.exit_code = 127;
        }
    }

    ControlFlow::Continue
}

fn uses_raw_builtin_args(command: &str) -> bool {
    matches!(command, "alias" | "export" | "set" | "unalias" | "unset")
}

fn expand_pipeline_aliases(
    shell: &Shell,
    pipeline: Pipeline,
) -> Result<Pipeline, shell::AliasError> {
    pipeline
        .commands
        .into_iter()
        .map(|command| expand_command_aliases(shell, command))
        .collect::<Result<Vec<_>, _>>()
        .map(|commands| Pipeline { commands })
}

fn expand_command_aliases(
    shell: &Shell,
    mut command: ParsedCommand,
) -> Result<ParsedCommand, shell::AliasError> {
    let mut seen = Vec::new();

    loop {
        let Some(replacement) = shell.alias_command(&command.name)? else {
            return Ok(command);
        };

        if let Some(index) = seen.iter().position(|name| name == &command.name) {
            seen.push(command.name);
            return Err(shell::AliasError::Cycle(seen[index..].to_vec()));
        }

        seen.push(command.name);

        let mut args = replacement.args;
        args.extend(command.args);

        command.name = replacement.name;
        command.args = args;
    }
}

fn raw_builtin_arg(argument: &Expression) -> Result<String, ShellError> {
    Ok(match argument {
        Expression::Identifier(name) => name.clone(),
        Expression::EnvironmentVariable(name) => format!("env.{name}"),
        Expression::Status => "status".into(),
        Expression::Literal(value) => value.to_string(),
        Expression::Binary {
            left,
            operator,
            right,
        } => format!(
            "{} {} {}",
            raw_builtin_arg(left)?,
            operator.symbol(),
            raw_builtin_arg(right)?
        ),
        Expression::Call { name, .. } => name.clone(),
        Expression::List(_) | Expression::Index { .. } | Expression::Len(_) => {
            return Err(ShellError::UnsupportedCall("complex raw argument".into()));
        }
    })
}

fn execute_if(
    shell: &mut Shell,
    branches: Vec<parser::IfBranch>,
    else_body: Option<Vec<ParsedInput>>,
) -> ControlFlow {
    for branch in branches {
        let condition = match evaluate_expression(shell, &branch.condition) {
            Ok(Value::Bool(value)) => value,
            Ok(value) => {
                eprintln!(
                    "crbsh: type mismatch: expected bool, found {}",
                    value.type_name()
                );
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
        };

        if condition {
            return execute_block(shell, branch.body);
        }
    }

    match else_body {
        Some(body) => execute_block(shell, body),
        None => {
            shell.exit_code = 0;
            ControlFlow::Continue
        }
    }
}

fn execute_match(
    shell: &mut Shell,
    value: parser::Expression,
    arms: Vec<parser::MatchArm>,
) -> ControlFlow {
    let value = match evaluate_expression(shell, &value) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }
    };

    for arm in arms {
        if pattern_matches(shell, &value, &arm.pattern) {
            return execute_input(shell, arm.body);
        }
    }

    shell.exit_code = 0;
    ControlFlow::Continue
}

fn execute_while(
    shell: &mut Shell,
    condition: parser::Expression,
    body: Vec<ParsedInput>,
) -> ControlFlow {
    loop {
        let condition = match evaluate_expression(shell, &condition) {
            Ok(Value::Bool(value)) => value,
            Ok(value) => {
                eprintln!(
                    "crbsh: type mismatch: expected bool, found {}",
                    value.type_name()
                );
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
                return ControlFlow::Continue;
            }
        };

        if !condition {
            shell.exit_code = 0;
            return ControlFlow::Continue;
        }

        match execute_block(shell, body.clone()) {
            ControlFlow::Continue => {}
            ControlFlow::LoopContinue => continue,
            ControlFlow::Break => {
                shell.exit_code = 0;
                return ControlFlow::Continue;
            }
            flow @ ControlFlow::Return(_) => return flow,
            flow @ ControlFlow::Exit(_) => return flow,
            flow @ ControlFlow::Error(_) => return flow,
        }
    }
}

fn execute_for(
    shell: &mut Shell,
    name: String,
    iterable: Iterable,
    body: Vec<ParsedInput>,
) -> ControlFlow {
    let values = match iterable_values(shell, iterable) {
        Ok(values) => values,
        Err(err) => {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }
    };

    for value in values {
        if let Err(err) = set_loop_variable(shell, &name, value) {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return ControlFlow::Continue;
        }

        match execute_block(shell, body.clone()) {
            ControlFlow::Continue => {}
            ControlFlow::LoopContinue => continue,
            ControlFlow::Break => {
                shell.exit_code = 0;
                return ControlFlow::Continue;
            }
            flow @ ControlFlow::Return(_) => return flow,
            flow @ ControlFlow::Exit(_) => return flow,
            flow @ ControlFlow::Error(_) => return flow,
        }
    }

    shell.exit_code = 0;
    ControlFlow::Continue
}

fn execute_block(shell: &mut Shell, body: Vec<ParsedInput>) -> ControlFlow {
    shell.push_scope();
    let mut result = ControlFlow::Continue;

    for statement in body {
        let flow = execute_input(shell, statement);

        if !matches!(flow, ControlFlow::Continue) {
            result = flow;
            break;
        }
    }

    shell.pop_scope();
    result
}

fn iterable_values(shell: &mut Shell, iterable: Iterable) -> Result<Vec<Value>, EvalError> {
    match iterable {
        Iterable::Range {
            start,
            end,
            inclusive,
        } => {
            let start = expect_int(evaluate_expression(shell, &start)?)?;
            let end = expect_int(evaluate_expression(shell, &end)?)?;
            let upper = if inclusive {
                end.saturating_add(1)
            } else {
                end
            };

            Ok((start..upper).map(Value::Int).collect())
        }
        Iterable::Glob(pattern) => Ok(glob_values(&pattern)
            .into_iter()
            .map(Value::String)
            .collect()),
        Iterable::Expression(expression) => match evaluate_expression(shell, &expression)? {
            Value::List(values) => Ok(values),
            value => Err(ShellError::TypeMismatch {
                expected: TypeName::List(None),
                found: value.type_name(),
            }
            .into()),
        },
    }
}

fn expect_int(value: Value) -> Result<i64, EvalError> {
    match value {
        Value::Int(value) => Ok(value),
        value => Err(shell::ShellError::TypeMismatch {
            expected: TypeName::Int,
            found: value.type_name(),
        }
        .into()),
    }
}

fn set_loop_variable(shell: &mut Shell, name: &str, value: Value) -> Result<(), shell::ShellError> {
    match shell.assign_variable(name, value.clone()) {
        Ok(()) => Ok(()),
        Err(shell::ShellError::VariableNotDefined(_)) => shell.declare_variable(name, None, value),
        Err(err) => Err(err),
    }
}

pub(crate) fn glob_values(pattern: &str) -> Vec<String> {
    if pattern.matches('*').count() != 1 {
        return Vec::new();
    }

    let path = Path::new(pattern);
    let Some(file_pattern) = path.file_name().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let Some((prefix, suffix)) = file_pattern.split_once('*') else {
        return Vec::new();
    };
    let directory = path.parent().filter(|path| !path.as_os_str().is_empty());
    let read_directory = directory.unwrap_or_else(|| Path::new("."));

    // v1 supports a single '*' in the file-name component, for example
    // '*.rs' or 'src/*.rs'. Recursive globs and multiple wildcards are ignored.
    let Ok(entries) = fs::read_dir(read_directory) else {
        return Vec::new();
    };

    let mut values = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && name.ends_with(suffix))
        .map(|name| {
            directory
                .map(|directory| directory.join(&name).to_string_lossy().into_owned())
                .unwrap_or(name)
        })
        .collect::<Vec<_>>();

    values.sort();
    values
}

fn pattern_matches(shell: &Shell, value: &Value, pattern: &parser::MatchPattern) -> bool {
    match pattern {
        parser::MatchPattern::Wildcard => true,
        parser::MatchPattern::Literal(pattern) => value == pattern,
        parser::MatchPattern::Status => value == &Value::Int(i64::from(shell.exit_code)),
        parser::MatchPattern::Identifier(name) => shell
            .evaluate(&parser::Expression::Identifier(name.clone()))
            .is_ok_and(|pattern| value == &pattern),
    }
}

pub(crate) fn evaluate_expression(
    shell: &mut Shell,
    expression: &parser::Expression,
) -> Result<Value, EvalError> {
    match expression {
        parser::Expression::Literal(value) => Ok(value.clone()),
        parser::Expression::Identifier(name) => shell
            .evaluate(&parser::Expression::Identifier(name.clone()))
            .map_err(Into::into),
        parser::Expression::EnvironmentVariable(name) => shell
            .environment_value(name)
            .map(Value::String)
            .ok_or_else(|| ShellError::UndefinedEnvironmentVariable(name.clone()).into()),
        parser::Expression::Status => Ok(Value::Int(i64::from(shell.exit_code))),
        parser::Expression::Binary {
            left,
            operator,
            right,
        } => {
            let left = evaluate_expression(shell, left)?;
            let right = evaluate_expression(shell, right)?;

            shell::evaluate_binary(*operator, left, right).map_err(Into::into)
        }
        parser::Expression::Call { name, args } => execute_function_call(shell, name, args)
            .and_then(|value| {
                value.ok_or_else(|| format!("function '{name}' did not return a value"))
            })
            .map_err(EvalError::Function),
        parser::Expression::List(expressions) => expressions
            .iter()
            .map(|expression| evaluate_expression(shell, expression))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|values| shell::validate_list_values(values).map_err(Into::into))
            .map(Value::List),
        parser::Expression::Index { target, index } => {
            let target = evaluate_expression(shell, target)?;
            let index = evaluate_expression(shell, index)?;
            shell::evaluate_index(target, index).map_err(Into::into)
        }
        parser::Expression::Len(target) => {
            let target = evaluate_expression(shell, target)?;
            shell::evaluate_len(target).map_err(Into::into)
        }
    }
}

pub(crate) fn execute_function_call(
    shell: &mut Shell,
    name: &str,
    args: &[parser::Expression],
) -> Result<Option<Value>, String> {
    shell.enter_function_call().map_err(|limit| {
        format!("function recursion limit of {limit} exceeded while calling '{name}'")
    })?;

    let result = execute_function_call_inner(shell, name, args);
    shell.exit_function_call();
    result
}

fn execute_function_call_inner(
    shell: &mut Shell,
    name: &str,
    args: &[parser::Expression],
) -> Result<Option<Value>, String> {
    let Some(function) = shell.function(name) else {
        return Err(format!("undefined function '{name}'"));
    };

    if args.len() != function.params.len() {
        return Err(format!(
            "function '{name}' expected {} arguments, found {}",
            function.params.len(),
            args.len()
        ));
    }

    let values = args
        .iter()
        .map(|arg| evaluate_expression(shell, arg).map_err(|err| err.to_string()))
        .collect::<Result<Vec<_>, _>>()?;

    let caller_scopes = shell.enter_function_scope();

    let mut setup_error = None;
    for (param, value) in function.params.iter().zip(values) {
        if let Some(expected) = &param.type_annotation
            && !expected.accepts(&value.type_name())
        {
            setup_error = Some(format!(
                "type mismatch: expected {expected}, found {}",
                value.type_name()
            ));
            break;
        }

        if let Err(err) = shell.declare_variable(&param.name, param.type_annotation.clone(), value)
        {
            setup_error = Some(err.to_string());
            break;
        }
    }

    if let Some(err) = setup_error {
        shell.restore_caller_scopes(caller_scopes);
        return Err(err);
    }

    let flow = execute_block(shell, function.body);
    shell.restore_caller_scopes(caller_scopes);

    match flow {
        ControlFlow::Return(value) => enforce_return_type(name, function.return_type, value),
        ControlFlow::Continue => {
            if let Some(return_type) = function.return_type {
                Err(format!("function '{name}' expected return {return_type}"))
            } else {
                Ok(None)
            }
        }
        ControlFlow::Break => Err("break outside loop".into()),
        ControlFlow::LoopContinue => Err("continue outside loop".into()),
        ControlFlow::Error(error) => Err(error),
        ControlFlow::Exit(code) => {
            shell.exit_code = code;
            Ok(None)
        }
    }
}

fn enforce_return_type(
    name: &str,
    return_type: Option<TypeName>,
    value: Option<Value>,
) -> Result<Option<Value>, String> {
    match (return_type, value) {
        (Some(expected), Some(value)) if expected.accepts(&value.type_name()) => Ok(Some(value)),
        (Some(expected), Some(value)) => Err(format!(
            "type mismatch: expected {expected}, found {}",
            value.type_name()
        )),
        (Some(expected), None) => Err(format!("function '{name}' expected return {expected}")),
        (None, value) => Ok(value),
    }
}
