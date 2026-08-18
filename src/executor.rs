use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::builtins;
use crate::parser::Pipeline;

#[derive(Debug)]
pub struct ExecutionError {
    pub command: String,
    pub source: io::Error,
}

pub fn execute_external(command: &str, args: &[String]) -> io::Result<i32> {
    let status = Command::new(command).args(args).status()?;

    Ok(status.code().unwrap_or(1))
}

pub fn execute_pipeline(pipeline: &Pipeline) -> Result<i32, ExecutionError> {
    if let Some(command) = pipeline.commands.first()
        && pipeline.commands.len() == 1
    {
        return execute_external(&command.name, &command.args).map_err(|source| ExecutionError {
            command: command.name.clone(),
            source,
        });
    }

    let mut children = Vec::new();
    let mut initial_input = None;
    let mut previous_stdout = None;
    let start_index = match pipeline.commands.first() {
        Some(command) if command.name == "print" => {
            initial_input = Some(builtins::print::output(&command.args).into_bytes());
            1
        }
        _ => 0,
    };
    let last_index = pipeline.commands.len().saturating_sub(1);

    for (index, command) in pipeline.commands.iter().enumerate().skip(start_index) {
        let mut process = Command::new(&command.name);
        process.args(&command.args);

        if initial_input.is_some() {
            process.stdin(Stdio::piped());
        } else if let Some(stdout) = previous_stdout.take() {
            process.stdin(Stdio::from(stdout));
        }

        if index != last_index {
            process.stdout(Stdio::piped());
        }

        let mut child = process.spawn().map_err(|source| ExecutionError {
            command: command.name.clone(),
            source,
        })?;

        if let Some(input) = initial_input.take()
            && let Some(mut stdin) = child.stdin.take()
        {
            stdin.write_all(&input).map_err(|source| ExecutionError {
                command: command.name.clone(),
                source,
            })?;
        }

        if index != last_index {
            previous_stdout = child.stdout.take();
        }

        children.push(child);
    }

    let mut exit_code = 0;

    for mut child in children {
        let status = child.wait().map_err(|source| ExecutionError {
            command: "<pipeline>".into(),
            source,
        })?;
        exit_code = status.code().unwrap_or(1);
    }

    Ok(exit_code)
}
