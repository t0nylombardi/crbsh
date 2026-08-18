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
use shell::Shell;
use value::Value;

enum ControlFlow {
    Continue,
    Break,
    LoopContinue,
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
        ParsedInput::Let {
            name,
            type_annotation,
            value,
        } => {
            let value = match shell.evaluate(&value) {
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

        ParsedInput::Assignment { name, value } => {
            let value = match shell.evaluate(&value) {
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

        ParsedInput::EnvironmentAssignment { name, value } => match shell.evaluate(&value) {
            Ok(value) => {
                shell.set_environment(name, value.to_string());
                shell.exit_code = 0;
            }
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
            }
        },

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
        let condition = match shell.evaluate(&branch.condition) {
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
    let value = match shell.evaluate(&value) {
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
        let condition = match shell.evaluate(&condition) {
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
            flow @ ControlFlow::Exit(_) => return flow,
        }
    }

    shell.exit_code = 0;
    ControlFlow::Continue
}

fn execute_block(shell: &mut Shell, body: Vec<ParsedInput>) -> ControlFlow {
    for statement in body {
        let flow = execute_input(shell, statement);

        if !matches!(flow, ControlFlow::Continue) {
            return flow;
        }
    }

    ControlFlow::Continue
}

fn iterable_values(shell: &Shell, iterable: Iterable) -> Result<Vec<Value>, shell::ShellError> {
    match iterable {
        Iterable::Range {
            start,
            end,
            inclusive,
        } => {
            let start = expect_int(shell.evaluate(&start)?)?;
            let end = expect_int(shell.evaluate(&end)?)?;
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

fn expect_int(value: Value) -> Result<i64, shell::ShellError> {
    match value {
        Value::Int(value) => Ok(value),
        value => Err(shell::ShellError::TypeMismatch {
            expected: value::TypeName::Int,
            found: value.type_name(),
        }),
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
