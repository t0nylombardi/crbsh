mod builtins;
mod executor;
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

        let mut parts = input.split_whitespace();

        let Some(command) = parts.next() else {
            continue;
        };

        let args: Vec<String> = parts.map(String::from).collect();

        if let Some(builtin) = shell.builtins.get(command) {
            match builtin(&mut shell, &args) {
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

        match executor::execute_external(command, &args) {
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
