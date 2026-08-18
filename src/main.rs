mod builtins;
mod executor;
mod parser;
mod prompt;
mod shell;
mod tokens;
mod value;

use std::fs;
use std::io::{self, Write};

use builtins::BuiltinOutcome;
use parser::{Iterable, ParsedInput};
use shell::{Shell, ShellError};
use value::TypeName;
use value::Value;

enum EvalError {
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

enum ControlFlow {
    Continue,
    Break,
    LoopContinue,
    Return(Option<Value>),
    Exit(i32),
}

fn main() {
    let mut shell = Shell::new();

    loop {
        print!("{}", prompt::render());
        io::stdout().flush().unwrap();

        let input = match read_input() {
            Ok(input) => input,
            Err(()) => continue,
        };

        if input.trim().is_empty() {
            continue;
        }

        let parsed_input = match parser::parse(&input) {
            Ok(parsed_input) => parsed_input,

            Err(err) => {
                eprintln!("crbsh: {}", parser::format_error(&err));
                shell.exit_code = 2;
                continue;
            }
        };

        match execute_input(&mut shell, parsed_input) {
            ControlFlow::Exit(code) => std::process::exit(code),
            ControlFlow::Break => {
                eprintln!("crbsh: break outside loop");
                shell.exit_code = 2;
            }
            ControlFlow::LoopContinue => {
                eprintln!("crbsh: continue outside loop");
                shell.exit_code = 2;
            }
            ControlFlow::Return(_) => {
                eprintln!("crbsh: return outside function");
                shell.exit_code = 2;
            }
            ControlFlow::Continue => {}
        }
    }
}

fn read_input() -> Result<String, ()> {
    let mut input = String::new();

    if io::stdin().read_line(&mut input).is_err() {
        return Err(());
    }

    while brace_balance(&input) > 0 {
        let mut next_line = String::new();

        if io::stdin().read_line(&mut next_line).is_err() {
            return Err(());
        }

        if next_line.is_empty() {
            break;
        }

        input.push_str(&next_line);
    }

    Ok(input)
}

fn brace_balance(input: &str) -> i32 {
    let mut balance = 0;
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quotes => escaped = true,
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes => in_double_quotes = !in_double_quotes,
            '{' if !in_single_quotes && !in_double_quotes => balance += 1,
            '}' if !in_single_quotes && !in_double_quotes => balance -= 1,
            _ => {}
        }
    }

    balance
}

fn execute_input(shell: &mut Shell, parsed_input: ParsedInput) -> ControlFlow {
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
                        eprintln!("crbsh: {err}");
                        shell.exit_code = 2;
                        return ControlFlow::Continue;
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

        ParsedInput::Pipeline(pipeline) => {
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
                let resolved_args = args
                    .iter()
                    .map(|arg| shell.resolve_argument(arg))
                    .collect::<Result<Vec<_>, _>>();

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

            match executor::execute_pipeline(shell, &pipeline) {
                Ok(code) => {
                    shell.exit_code = code;
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
    }
}

fn expect_int(value: Value) -> Result<i64, EvalError> {
    match value {
        Value::Int(value) => Ok(value),
        value => Err(shell::ShellError::TypeMismatch {
            expected: value::TypeName::Int,
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

fn glob_values(pattern: &str) -> Vec<String> {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return Vec::new();
    };

    let Ok(entries) = fs::read_dir(".") else {
        return Vec::new();
    };

    let mut values = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && name.ends_with(suffix))
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

fn evaluate_expression(
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

            Shell::evaluate_binary(*operator, left, right).map_err(Into::into)
        }
        parser::Expression::Call { name, args } => execute_function_call(shell, name, args)
            .and_then(|value| {
                value.ok_or_else(|| format!("function '{name}' did not return a value"))
            })
            .map_err(EvalError::Function),
    }
}

fn execute_function_call(
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

    shell.push_scope();

    let mut setup_error = None;
    for (param, value) in function.params.iter().zip(values) {
        if value.type_name() != param.type_name {
            setup_error = Some(format!(
                "type mismatch: expected {}, found {}",
                param.type_name,
                value.type_name()
            ));
            break;
        }

        if let Err(err) = shell.declare_variable(&param.name, Some(param.type_name), value) {
            setup_error = Some(err.to_string());
            break;
        }
    }

    if let Some(err) = setup_error {
        shell.pop_scope();
        return Err(err);
    }

    let flow = execute_block(shell, function.body);
    shell.pop_scope();

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
        (Some(expected), Some(value)) if value.type_name() == expected => Ok(Some(value)),
        (Some(expected), Some(value)) => Err(format!(
            "type mismatch: expected {expected}, found {}",
            value.type_name()
        )),
        (Some(expected), None) => Err(format!("function '{name}' expected return {expected}")),
        (None, value) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::Expression;

    use super::*;

    #[test]
    fn function_call_expression_returns_value() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn add(a: int, b: int) -> int {
    return a + b
}
"#,
        );
        run(&mut shell, "let total = add(2, 3)");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("total".into())),
            Ok(Value::Int(5))
        );
    }

    #[test]
    fn function_invocation_uses_fresh_scope() {
        let mut shell = Shell::new();

        run(&mut shell, "let x = 10");
        run(
            &mut shell,
            r#"
fn test(x: int) -> int {
    let y = 5
    return x
}
"#,
        );
        run(&mut shell, "let result = test(20)");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("x".into())),
            Ok(Value::Int(10))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("result".into())),
            Ok(Value::Int(20))
        );
        assert!(matches!(
            shell.evaluate(&Expression::Identifier("y".into())),
            Err(ShellError::UndefinedVariable(_))
        ));
    }

    #[test]
    fn function_returns_from_nested_block() {
        let mut shell = Shell::new();

        run(
            &mut shell,
            r#"
fn find_positive(x: int) -> int {
    if x > 0 {
        return x
    }

    return 0
}
"#,
        );
        run(&mut shell, "let positive = find_positive(3)");
        run(&mut shell, "let fallback = find_positive(0)");

        assert_eq!(
            shell.evaluate(&Expression::Identifier("positive".into())),
            Ok(Value::Int(3))
        );
        assert_eq!(
            shell.evaluate(&Expression::Identifier("fallback".into())),
            Ok(Value::Int(0))
        );
    }

    fn run(shell: &mut Shell, input: &str) {
        let parsed = parser::parse(input).unwrap();
        assert!(matches!(
            execute_input(shell, parsed),
            ControlFlow::Continue
        ));
        assert_eq!(shell.exit_code, 0);
    }
}
