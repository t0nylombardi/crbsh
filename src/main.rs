mod builtins;
mod executor;
mod parser;
mod prompt;
mod shell;

use std::io::{self, Write};

use builtins::BuiltinOutcome;
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

        let parsed = match parser::parse(input) {
            Ok(parsed) => parsed,

            Err(err) => {
                eprintln!("crbsh: parse error: {err:?}");
                shell.exit_code = 2;
                continue;
            }
        };

        let command = &parsed.name;
        let args = &parsed.args;

        if let Some(builtin) = shell.builtins.get(command) {
            match builtin(&mut shell, args) {
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

        match executor::execute_external(command, args) {
            Ok(code) => {
                shell.exit_code = code;
            }

            Err(err) => {
                eprintln!("crbsh: {command}: {err}");
                shell.exit_code = 127;
            }
        }
    }
}
