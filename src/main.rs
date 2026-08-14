mod builtins;
use std::io::{self, Write};
use std::process::Command;

fn main() {
    loop {
        print!("crbsh> ");
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
        let command = parts.next().unwrap();
        let args: Vec<&str> = parts.collect();

        match command {
            "exit" => {
                if builtins::exit::run() {
                    break;
                }
            }

            "cd" => {
                builtins::cd::run(&args);
            }

            "print" => {
                builtins::print::run(&args);
            }

            _ => match Command::new(command).args(args).status() {
                Ok(_) => {}
                Err(err) => {
                    eprintln!("crbsh: {command}: {err}");
                }
            },
        }
    }
}
