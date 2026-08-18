mod builtins;
mod executor;
mod parser;
mod prompt;
mod shell;
mod tokens;
mod value;

use std::io::{self, Write};

use builtins::BuiltinOutcome;
use parser::ParsedInput;
use shell::Shell;
use value::Value;

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

        if let Some(code) = execute_input(&mut shell, parsed_input) {
            std::process::exit(code);
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

fn execute_input(shell: &mut Shell, parsed_input: ParsedInput) -> Option<i32> {
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
                    return None;
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

        ParsedInput::Assignment { name, value } => {
            let value = match shell.evaluate(&value) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("crbsh: {err}");
                    shell.exit_code = 2;
                    return None;
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
            if let Some(code) = execute_if(shell, branches, else_body) {
                return Some(code);
            }
        }

        ParsedInput::Match { value, arms } => {
            if let Some(code) = execute_match(shell, value, arms) {
                return Some(code);
            }
        }

        ParsedInput::Pipeline(pipeline) => {
            let parsed = match pipeline.commands.first() {
                Some(command) => command,
                None => {
                    shell.exit_code = 0;
                    return None;
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
                        return None;
                    }
                };

                match builtin(shell, &resolved_args) {
                    Ok(BuiltinOutcome::Continue) => {
                        shell.exit_code = 0;
                    }

                    Ok(BuiltinOutcome::Exit(code)) => {
                        return Some(code);
                    }

                    Err(err) => {
                        eprintln!("crbsh: {}", err.message);
                        shell.exit_code = 1;
                    }
                }

                return None;
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

    None
}

fn execute_if(
    shell: &mut Shell,
    branches: Vec<parser::IfBranch>,
    else_body: Option<Vec<ParsedInput>>,
) -> Option<i32> {
    for branch in branches {
        let condition = match shell.evaluate(&branch.condition) {
            Ok(Value::Bool(value)) => value,
            Ok(value) => {
                eprintln!(
                    "crbsh: type mismatch: expected bool, found {}",
                    value.type_name()
                );
                shell.exit_code = 2;
                return None;
            }
            Err(err) => {
                eprintln!("crbsh: {err}");
                shell.exit_code = 2;
                return None;
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
            None
        }
    }
}

fn execute_match(
    shell: &mut Shell,
    value: parser::Expression,
    arms: Vec<parser::MatchArm>,
) -> Option<i32> {
    let value = match shell.evaluate(&value) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("crbsh: {err}");
            shell.exit_code = 2;
            return None;
        }
    };

    for arm in arms {
        if pattern_matches(shell, &value, &arm.pattern) {
            return execute_input(shell, arm.body);
        }
    }

    shell.exit_code = 0;
    None
}

fn execute_block(shell: &mut Shell, body: Vec<ParsedInput>) -> Option<i32> {
    for statement in body {
        if let Some(code) = execute_input(shell, statement) {
            return Some(code);
        }
    }

    None
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
