use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::builtins;
use crate::parser::ParsedCommand;
use crate::shell::Shell;

use super::redirect::output_file;
use super::{ExecutionError, status_exit_code};

pub(super) fn external_process(
    shell: &Shell,
    command: &ParsedCommand,
) -> Result<Command, ExecutionError> {
    let mut process = Command::new(&command.name);
    process
        .args(resolved_args(shell, command)?)
        .envs(shell.environment_overrides());

    if let Some(path) = &command.redirections.stdin {
        let input = File::open(path).map_err(|source| ExecutionError {
            command: command.name.clone(),
            message: source.to_string(),
        })?;
        process.stdin(Stdio::from(input));
    }

    if let Some(output) = command.redirections.stdout.as_ref() {
        process.stdout(Stdio::from(output_file(
            command,
            &output.target,
            output.append,
        )?));
    }

    Ok(process)
}

pub(super) fn execute_single_external(
    shell: &Shell,
    command: &ParsedCommand,
) -> Result<i32, ExecutionError> {
    let mut process = external_process(shell, command)?;

    let status = process.status().map_err(|source| ExecutionError {
        command: command.name.clone(),
        message: source.to_string(),
    })?;

    Ok(status_exit_code(status))
}

pub(super) fn execute_print(shell: &Shell, command: &ParsedCommand) -> Result<i32, ExecutionError> {
    let output = print_output(shell, command)?;

    if let Some(redirection) = command.redirections.stdout.as_ref() {
        let mut file = output_file(command, &redirection.target, redirection.append)?;

        file.write_all(output.as_bytes())
            .map_err(|source| ExecutionError {
                command: command.name.clone(),
                message: source.to_string(),
            })?;
    } else {
        print!("{output}");
    }

    Ok(0)
}

pub(super) fn print_output(
    shell: &Shell,
    command: &ParsedCommand,
) -> Result<String, ExecutionError> {
    let args = resolved_args(shell, command)?;

    Ok(builtins::print::output(&args))
}

pub(super) fn resolved_args(
    shell: &Shell,
    command: &ParsedCommand,
) -> Result<Vec<String>, ExecutionError> {
    command
        .args
        .iter()
        .map(|arg| shell.resolve_argument(arg).map_err(ExecutionError::from))
        .collect()
}
