use std::fs::{File, OpenOptions};

use crate::parser::ParsedCommand;

use super::ExecutionError;

pub(super) fn output_file(
    command: &ParsedCommand,
    path: &str,
    append: bool,
) -> Result<File, ExecutionError> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|source| ExecutionError {
            command: command.name.clone(),
            message: source.to_string(),
        })
}
