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

fn main() {
    let mut shell = Shell::new();

    loop {
        print!("{}", prompt::render());
        io::stdout().flush().unwrap();

        let mut input = String::new();

        if io::stdin().read_line(&mut input).is_err() {
            continue;
        }

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        let parsed_input = match parser::parse(input) {
            Ok(parsed_input) => parsed_input,

            Err(err) => {
                eprintln!("crbsh: {}", parser::format_error(&err));
                shell.exit_code = 2;
                continue;
            }
        };

        let pipeline = match parsed_input {
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
                        continue;
                    }
                };

                match shell.declare_variable(name, type_annotation, value) {
                    Ok(()) => shell.exit_code = 0,
                    Err(err) => {
                        eprintln!("crbsh: {err}");
                        shell.exit_code = 2;
                    }
                }

                continue;
            }

            ParsedInput::Assignment { name, value } => {
                let value = match shell.evaluate(&value) {
                    Ok(value) => value,
                    Err(err) => {
                        eprintln!("crbsh: {err}");
                        shell.exit_code = 2;
                        continue;
                    }
                };

                match shell.assign_variable(name, value) {
                    Ok(()) => shell.exit_code = 0,
                    Err(err) => {
                        eprintln!("crbsh: {err}");
                        shell.exit_code = 2;
                    }
                }

                continue;
            }

            ParsedInput::EnvironmentAssignment { name, value } => {
                match shell.evaluate(&value) {
                    Ok(value) => {
                        shell.set_environment(name, value.to_string());
                        shell.exit_code = 0;
                    }
                    Err(err) => {
                        eprintln!("crbsh: {err}");
                        shell.exit_code = 2;
                    }
                }

                continue;
            }

            ParsedInput::Pipeline(pipeline) => pipeline,
        };

        let parsed = match pipeline.commands.first() {
            Some(command) => command,
            None => {
                shell.exit_code = 0;
                continue;
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
                    continue;
                }
            };

            match builtin(&mut shell, &resolved_args) {
                Ok(BuiltinOutcome::Continue) => {
                    shell.exit_code = 0;
                }

                Ok(BuiltinOutcome::Exit(code)) => {
                    std::process::exit(code);
                }

                Err(err) => {
                    eprintln!("crbsh: {}", err.message);
                    shell.exit_code = 1;
                }
            }

            continue;
        }

        match executor::execute_pipeline(&shell, &pipeline) {
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
