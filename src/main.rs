mod builtins;
mod executor;
mod parser;
mod prompt;
mod shell;
mod tokens;

use std::io::{self, Write};

use builtins::BuiltinOutcome;
use parser::ParsedInput;
use shell::{NativeValue, Shell};

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
            ParsedInput::Let { name, value } => {
                shell.set_variable(name, NativeValue::parse(&value));
                shell.exit_code = 0;
                continue;
            }

            ParsedInput::EnvironmentAssignment { name, value } => {
                let value = shell.resolve_word(&value);
                shell.set_environment(name, value);
                shell.exit_code = 0;
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
                .map(|arg| shell.resolve_word(arg))
                .collect::<Vec<_>>();

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
                eprintln!("crbsh: {}: {}", err.command, err.source);
                shell.exit_code = 127;
            }
        }
    }
}
