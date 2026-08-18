use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::builtins;
use crate::parser::{ParsedCommand, Pipeline};

#[derive(Debug)]
pub struct ExecutionError {
    pub command: String,
    pub source: io::Error,
}

pub fn execute_pipeline(pipeline: &Pipeline) -> Result<i32, ExecutionError> {
    if pipeline.commands.len() == 1 {
        let command = &pipeline.commands[0];

        if command.name == "print" {
            return execute_print(command);
        }

        return execute_single_external(command);
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

        if let Some(path) = &command.redirections.stdin {
            let input = File::open(path).map_err(|source| ExecutionError {
                command: command.name.clone(),
                source,
            })?;
            process.stdin(Stdio::from(input));
        } else if initial_input.is_some() {
            process.stdin(Stdio::piped());
        } else if let Some(stdout) = previous_stdout.take() {
            process.stdin(Stdio::from(stdout));
        }

        if let Some(output) = command.redirections.stdout.as_ref() {
            process.stdout(Stdio::from(output_file(
                command,
                &output.target,
                output.append,
            )?));
        } else if index != last_index {
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

fn execute_single_external(command: &ParsedCommand) -> Result<i32, ExecutionError> {
    let mut process = Command::new(&command.name);
    process.args(&command.args);

    if let Some(path) = &command.redirections.stdin {
        let input = File::open(path).map_err(|source| ExecutionError {
            command: command.name.clone(),
            source,
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

    let status = process.status().map_err(|source| ExecutionError {
        command: command.name.clone(),
        source,
    })?;

    Ok(status.code().unwrap_or(1))
}

fn execute_print(command: &ParsedCommand) -> Result<i32, ExecutionError> {
    let output = builtins::print::output(&command.args);

    if let Some(redirection) = command.redirections.stdout.as_ref() {
        let mut file = output_file(command, &redirection.target, redirection.append)?;

        file.write_all(output.as_bytes())
            .map_err(|source| ExecutionError {
                command: command.name.clone(),
                source,
            })?;
    } else {
        print!("{output}");
    }

    Ok(0)
}

fn output_file(command: &ParsedCommand, path: &str, append: bool) -> Result<File, ExecutionError> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|source| ExecutionError {
            command: command.name.clone(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::parser::{OutputRedirection, ParsedCommand, Pipeline, Redirections};

    use super::execute_pipeline;

    #[test]
    fn redirects_print_output_to_file() {
        let dir = temp_dir("redirects_print_output_to_file");
        let output = dir.join("out.txt");

        let code = execute_pipeline(&Pipeline {
            commands: vec![ParsedCommand {
                name: "print".into(),
                args: vec!["hello".into()],
                redirections: Redirections {
                    stdin: None,
                    stdout: Some(OutputRedirection {
                        target: output.to_string_lossy().into_owned(),
                        append: false,
                    }),
                },
            }],
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "hello\n");
    }

    #[test]
    fn appends_print_output_to_file() {
        let dir = temp_dir("appends_print_output_to_file");
        let output = dir.join("out.txt");

        fs::write(&output, "first\n").unwrap();

        let code = execute_pipeline(&Pipeline {
            commands: vec![ParsedCommand {
                name: "print".into(),
                args: vec!["second".into()],
                redirections: Redirections {
                    stdin: None,
                    stdout: Some(OutputRedirection {
                        target: output.to_string_lossy().into_owned(),
                        append: true,
                    }),
                },
            }],
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "first\nsecond\n");
    }

    #[test]
    fn redirects_external_input_and_output() {
        let dir = temp_dir("redirects_external_input_and_output");
        let input = dir.join("input.txt");
        let output = dir.join("output.txt");

        fs::write(&input, "crab\nstone crab\nfish\n").unwrap();

        let code = execute_pipeline(&Pipeline {
            commands: vec![ParsedCommand {
                name: "grep".into(),
                args: vec!["crab".into()],
                redirections: Redirections {
                    stdin: Some(input.to_string_lossy().into_owned()),
                    stdout: Some(OutputRedirection {
                        target: output.to_string_lossy().into_owned(),
                        append: false,
                    }),
                },
            }],
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "crab\nstone crab\n");
    }

    #[test]
    fn redirects_pipeline_output() {
        let dir = temp_dir("redirects_pipeline_output");
        let output = dir.join("results.txt");

        let code = execute_pipeline(&Pipeline {
            commands: vec![
                ParsedCommand {
                    name: "print".into(),
                    args: vec!["parser".into()],
                    redirections: Redirections::default(),
                },
                ParsedCommand {
                    name: "grep".into(),
                    args: vec!["pars".into()],
                    redirections: Redirections {
                        stdin: None,
                        stdout: Some(OutputRedirection {
                            target: output.to_string_lossy().into_owned(),
                            append: false,
                        }),
                    },
                },
            ],
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "parser\n");
    }

    fn temp_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("crbsh-{test_name}-{nanos}"));

        fs::create_dir(&dir).unwrap();

        dir
    }
}
