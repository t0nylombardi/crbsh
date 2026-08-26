use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

use crate::builtins;
use crate::jobs::JobId;
use crate::parser::{ParsedCommand, Pipeline};
use crate::shell::{Shell, ShellError};

#[derive(Debug)]
pub struct ExecutionError {
    pub command: String,
    pub message: String,
}

pub fn execute_pipeline(shell: &Shell, pipeline: &Pipeline) -> Result<i32, ExecutionError> {
    if pipeline.commands.len() == 1 {
        let command = &pipeline.commands[0];

        if command.name == "print" {
            return execute_print(shell, command);
        }

        return execute_single_external(shell, command);
    }

    let mut children = ChildCleanup::new();
    let mut initial_input = None;
    let mut initial_stdin = None;
    let mut previous_stdout = None;
    let start_index = match pipeline.commands.first() {
        Some(command) if command.name == "print" => {
            initial_input = Some(print_output(shell, command)?.into_bytes());
            1
        }
        _ => 0,
    };
    let last_index = pipeline.commands.len().saturating_sub(1);

    for (index, command) in pipeline.commands.iter().enumerate().skip(start_index) {
        let uses_initial_input = initial_input.is_some() && index == start_index;

        if shell.builtins.get(&command.name).is_some() {
            return Err(ExecutionError {
                command: command.name.clone(),
                message: "builtins are not supported in this pipeline position".into(),
            });
        }

        let mut process = Command::new(&command.name);
        let args = match resolved_args(shell, command) {
            Ok(args) => args,
            Err(err) => {
                children.cleanup();
                return Err(err);
            }
        };

        process.args(args).envs(shell.environment_overrides());

        if let Some(path) = &command.redirections.stdin {
            let input = match File::open(path) {
                Ok(input) => input,
                Err(source) => {
                    children.cleanup();
                    return Err(ExecutionError {
                        command: command.name.clone(),
                        message: source.to_string(),
                    });
                }
            };
            process.stdin(Stdio::from(input));
        } else if uses_initial_input {
            process.stdin(Stdio::piped());
        } else if let Some(stdout) = previous_stdout.take() {
            process.stdin(Stdio::from(stdout));
        }

        if let Some(output) = command.redirections.stdout.as_ref() {
            let output = match output_file(command, &output.target, output.append) {
                Ok(output) => output,
                Err(err) => {
                    children.cleanup();
                    return Err(err);
                }
            };
            process.stdout(Stdio::from(output));
        } else if index != last_index {
            process.stdout(Stdio::piped());
        }

        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(source) => {
                children.cleanup();
                return Err(ExecutionError {
                    command: command.name.clone(),
                    message: source.to_string(),
                });
            }
        };

        if uses_initial_input {
            initial_stdin = child.stdin.take();
        }

        if index != last_index {
            previous_stdout = child.stdout.take();
        }

        children.push(child);
    }

    if let Some(input) = initial_input.take()
        && let Some(mut stdin) = initial_stdin.take()
        && let Err(source) = stdin.write_all(&input)
    {
        children.cleanup();
        return Err(ExecutionError {
            command: "<pipeline>".into(),
            message: source.to_string(),
        });
    }

    let mut exit_code = 0;

    for mut child in children.into_children() {
        let status = child.wait().map_err(|source| ExecutionError {
            command: "<pipeline>".into(),
            message: source.to_string(),
        })?;
        exit_code = status_exit_code(status);
    }

    Ok(exit_code)
}

pub fn execute_background_pipeline(
    shell: &mut Shell,
    pipeline: &Pipeline,
    command: String,
) -> Result<(JobId, u32), ExecutionError> {
    let mut children = spawn_pipeline(shell, pipeline, true)?;

    if children.is_empty() {
        return Err(ExecutionError {
            command,
            message: "background jobs require an external command".into(),
        });
    }

    let pid = children
        .first()
        .map(std::process::Child::id)
        .expect("checked background pipeline has at least one child");
    let id = shell.jobs.add(command, std::mem::take(&mut children));

    Ok((id, pid))
}

fn spawn_pipeline(
    shell: &Shell,
    pipeline: &Pipeline,
    background: bool,
) -> Result<Vec<std::process::Child>, ExecutionError> {
    if pipeline.commands.len() == 1 {
        let command = &pipeline.commands[0];

        if shell.builtins.get(&command.name).is_some() {
            return Err(ExecutionError {
                command: command.name.clone(),
                message: "builtins cannot be run as background jobs".into(),
            });
        }

        let mut process = external_process(shell, command)?;

        if background && command.redirections.stdin.is_none() {
            process.stdin(Stdio::null());
        }

        return process
            .spawn()
            .map(|child| vec![child])
            .map_err(|source| ExecutionError {
                command: command.name.clone(),
                message: source.to_string(),
            });
    }

    let mut children = ChildCleanup::new();
    let mut initial_input = None;
    let mut initial_stdin = None;
    let mut previous_stdout = None;
    let start_index = match pipeline.commands.first() {
        Some(command) if command.name == "print" => {
            initial_input = Some(print_output(shell, command)?.into_bytes());
            1
        }
        _ => 0,
    };
    let last_index = pipeline.commands.len().saturating_sub(1);

    for (index, command) in pipeline.commands.iter().enumerate().skip(start_index) {
        let uses_initial_input = initial_input.is_some() && index == start_index;

        if shell.builtins.get(&command.name).is_some() {
            return Err(ExecutionError {
                command: command.name.clone(),
                message: "builtins are not supported in this pipeline position".into(),
            });
        }

        let mut process = Command::new(&command.name);
        let args = match resolved_args(shell, command) {
            Ok(args) => args,
            Err(err) => {
                children.cleanup();
                return Err(err);
            }
        };

        process.args(args).envs(shell.environment_overrides());

        if let Some(path) = &command.redirections.stdin {
            let input = match File::open(path) {
                Ok(input) => input,
                Err(source) => {
                    children.cleanup();
                    return Err(ExecutionError {
                        command: command.name.clone(),
                        message: source.to_string(),
                    });
                }
            };
            process.stdin(Stdio::from(input));
        } else if uses_initial_input {
            process.stdin(Stdio::piped());
        } else if let Some(stdout) = previous_stdout.take() {
            process.stdin(Stdio::from(stdout));
        } else if background && index == start_index {
            process.stdin(Stdio::null());
        }

        if let Some(output) = command.redirections.stdout.as_ref() {
            let output = match output_file(command, &output.target, output.append) {
                Ok(output) => output,
                Err(err) => {
                    children.cleanup();
                    return Err(err);
                }
            };
            process.stdout(Stdio::from(output));
        } else if index != last_index {
            process.stdout(Stdio::piped());
        }

        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(source) => {
                children.cleanup();
                return Err(ExecutionError {
                    command: command.name.clone(),
                    message: source.to_string(),
                });
            }
        };

        if uses_initial_input {
            initial_stdin = child.stdin.take();
        }

        if index != last_index {
            previous_stdout = child.stdout.take();
        }

        children.push(child);
    }

    if let Some(input) = initial_input.take()
        && let Some(mut stdin) = initial_stdin.take()
        && let Err(source) = stdin.write_all(&input)
    {
        children.cleanup();
        return Err(ExecutionError {
            command: "<pipeline>".into(),
            message: source.to_string(),
        });
    }

    Ok(children.into_children())
}

struct ChildCleanup {
    children: Vec<std::process::Child>,
}

impl ChildCleanup {
    fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    fn push(&mut self, child: std::process::Child) {
        self.children.push(child);
    }

    fn cleanup(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn into_children(mut self) -> Vec<std::process::Child> {
        std::mem::take(&mut self.children)
    }
}

impl Drop for ChildCleanup {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn status_exit_code(status: std::process::ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

fn external_process(shell: &Shell, command: &ParsedCommand) -> Result<Command, ExecutionError> {
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

fn execute_single_external(shell: &Shell, command: &ParsedCommand) -> Result<i32, ExecutionError> {
    let mut process = external_process(shell, command)?;

    let status = process.status().map_err(|source| ExecutionError {
        command: command.name.clone(),
        message: source.to_string(),
    })?;

    Ok(status_exit_code(status))
}

fn execute_print(shell: &Shell, command: &ParsedCommand) -> Result<i32, ExecutionError> {
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

fn print_output(shell: &Shell, command: &ParsedCommand) -> Result<String, ExecutionError> {
    let args = resolved_args(shell, command)?;

    Ok(builtins::print::output(&args))
}

fn resolved_args(shell: &Shell, command: &ParsedCommand) -> Result<Vec<String>, ExecutionError> {
    command
        .args
        .iter()
        .map(|arg| shell.resolve_argument(arg).map_err(ExecutionError::from))
        .collect()
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
            message: source.to_string(),
        })
}

impl From<ShellError> for ExecutionError {
    fn from(error: ShellError) -> Self {
        Self {
            command: "evaluation".into(),
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::jobs::JobState;
    use crate::parser::{OutputRedirection, ParsedCommand, Pipeline, Redirections};
    use crate::runtime::Value;
    use crate::shell::Shell;

    use super::{execute_background_pipeline, execute_pipeline, status_exit_code};

    #[test]
    fn redirects_print_output_to_file() {
        let dir = temp_dir("redirects_print_output_to_file");
        let output = dir.join("out.txt");

        let shell = Shell::new();

        let code = execute_pipeline(
            &shell,
            &Pipeline {
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
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "hello\n");
    }

    #[test]
    fn appends_print_output_to_file() {
        let dir = temp_dir("appends_print_output_to_file");
        let output = dir.join("out.txt");

        fs::write(&output, "first\n").unwrap();

        let shell = Shell::new();

        let code = execute_pipeline(
            &shell,
            &Pipeline {
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
            },
        )
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

        let shell = Shell::new();

        let code = execute_pipeline(
            &shell,
            &Pipeline {
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
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "crab\nstone crab\n");
    }

    #[test]
    fn redirects_pipeline_output() {
        let dir = temp_dir("redirects_pipeline_output");
        let output = dir.join("results.txt");

        let shell = Shell::new();

        let code = execute_pipeline(
            &shell,
            &Pipeline {
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
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "parser\n");
    }

    #[test]
    fn resolves_print_arguments_from_shell_values() {
        let dir = temp_dir("resolves_print_arguments_from_shell_values");
        let output = dir.join("out.txt");
        let mut shell = Shell::new();

        shell.set_variable("project", Value::String("crbsh".into()));
        shell.set_variable("retries", Value::Int(3));
        shell.set_environment("CRBSH_TEST_ENV", "debug");
        shell.exit_code = 42;

        let code = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![ParsedCommand {
                    name: "print".into(),
                    args: vec![
                        "project".into(),
                        "retries".into(),
                        "@CRBSH_TEST_ENV".into(),
                        "env.CRBSH_TEST_ENV".into(),
                        "status".into(),
                    ],
                    redirections: Redirections {
                        stdin: None,
                        stdout: Some(OutputRedirection {
                            target: output.to_string_lossy().into_owned(),
                            append: false,
                        }),
                    },
                }],
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            "crbsh 3 debug debug 42\n"
        );
    }

    #[test]
    fn keeps_quoted_identifier_literal() {
        let dir = temp_dir("keeps_quoted_identifier_literal");
        let output = dir.join("out.txt");
        let mut shell = Shell::new();

        shell.set_variable("project", Value::String("crbsh".into()));

        let code = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![ParsedCommand {
                    name: "print".into(),
                    args: vec![Value::String("project".into()).into()],
                    redirections: Redirections {
                        stdin: None,
                        stdout: Some(OutputRedirection {
                            target: output.to_string_lossy().into_owned(),
                            append: false,
                        }),
                    },
                }],
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(fs::read_to_string(output).unwrap(), "project\n");
    }

    #[test]
    fn reports_undefined_environment_variable_in_arguments() {
        let shell = Shell::new();
        let error = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![ParsedCommand {
                    name: "print".into(),
                    args: vec![crate::parser::Expression::EnvironmentVariable(
                        "CRBSH_DEFINITELY_MISSING".into(),
                    )],
                    redirections: Redirections::default(),
                }],
            },
        )
        .unwrap_err();

        assert_eq!(error.command, "evaluation");
        assert_eq!(
            error.message,
            "undefined environment variable 'CRBSH_DEFINITELY_MISSING'"
        );
    }

    #[test]
    fn exports_environment_overrides_to_external_pipeline_processes() {
        let dir = temp_dir("exports_environment_overrides_to_external_pipeline_processes");
        let output = dir.join("out.txt");
        let mut shell = Shell::new();

        shell.set_environment("CRBSH_PIPE_ENV", "debug");

        let code = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![
                    ParsedCommand {
                        name: "/usr/bin/env".into(),
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    },
                    ParsedCommand {
                        name: "grep".into(),
                        args: vec![Value::String("CRBSH_PIPE_ENV=debug".into()).into()],
                        redirections: Redirections {
                            stdin: None,
                            stdout: Some(OutputRedirection {
                                target: output.to_string_lossy().into_owned(),
                                append: false,
                            }),
                        },
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            "CRBSH_PIPE_ENV=debug\n"
        );
    }

    #[test]
    fn pipeline_exit_code_comes_from_last_stage() {
        let shell = Shell::new();

        let earlier_failure_code = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![
                    ParsedCommand {
                        name: "false".into(),
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    },
                    ParsedCommand {
                        name: "true".into(),
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    },
                ],
            },
        )
        .unwrap();

        let last_failure_code = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![
                    ParsedCommand {
                        name: "true".into(),
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    },
                    ParsedCommand {
                        name: "false".into(),
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(earlier_failure_code, 0);
        assert_eq!(last_failure_code, 1);
    }

    #[test]
    fn reports_missing_command_failure() {
        let shell = Shell::new();
        let error = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![ParsedCommand {
                    name: "crbsh-definitely-missing-command".into(),
                    args: Vec::new(),
                    redirections: Redirections::default(),
                }],
            },
        )
        .unwrap_err();

        assert_eq!(error.command, "crbsh-definitely-missing-command");
        assert!(!error.message.is_empty());
    }

    #[test]
    fn rejects_builtin_in_non_leading_pipeline_position() {
        let shell = Shell::new();
        let error = execute_pipeline(
            &shell,
            &Pipeline {
                commands: vec![
                    ParsedCommand {
                        name: "true".into(),
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    },
                    ParsedCommand {
                        name: "print".into(),
                        args: Vec::new(),
                        redirections: Redirections::default(),
                    },
                ],
            },
        )
        .unwrap_err();

        assert_eq!(error.command, "print");
        assert_eq!(
            error.message,
            "builtins are not supported in this pipeline position"
        );
    }

    #[test]
    fn starts_background_external_command_without_waiting() {
        let mut shell = Shell::new();

        let (id, pid) = execute_background_pipeline(
            &mut shell,
            &Pipeline {
                commands: vec![ParsedCommand {
                    name: "sleep".into(),
                    args: vec![Value::String("0.1".into()).into()],
                    redirections: Redirections::default(),
                }],
            },
            "sleep 0.1".into(),
        )
        .unwrap();

        assert_eq!(id, 1);
        assert!(pid > 0);

        let statuses = shell.jobs.statuses();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, JobState::Running);
        assert_eq!(statuses[0].command, "sleep 0.1");
    }

    #[test]
    fn starts_background_pipeline() {
        let mut shell = Shell::new();

        let (id, pid) = execute_background_pipeline(
            &mut shell,
            &Pipeline {
                commands: vec![
                    ParsedCommand {
                        name: "printf".into(),
                        args: vec![Value::String("crab\n".into()).into()],
                        redirections: Redirections::default(),
                    },
                    ParsedCommand {
                        name: "grep".into(),
                        args: vec!["crab".into()],
                        redirections: Redirections::default(),
                    },
                ],
            },
            "printf crab | grep crab".into(),
        )
        .unwrap();

        assert_eq!(id, 1);
        assert!(pid > 0);
        assert_eq!(shell.jobs.statuses()[0].command, "printf crab | grep crab");
    }

    #[test]
    fn rejects_background_builtin() {
        let mut shell = Shell::new();

        let error = execute_background_pipeline(
            &mut shell,
            &Pipeline {
                commands: vec![ParsedCommand {
                    name: "jobs".into(),
                    args: Vec::new(),
                    redirections: Redirections::default(),
                }],
            },
            "jobs".into(),
        )
        .unwrap_err();

        assert_eq!(error.command, "jobs");
        assert_eq!(error.message, "builtins cannot be run as background jobs");
    }

    #[test]
    fn signal_status_maps_to_128_plus_signal() {
        let mut child = Command::new("sleep").arg("10").spawn().unwrap();

        child.kill().unwrap();
        let status = child.wait().unwrap();

        assert_eq!(status_exit_code(status), 137);
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn join(&self, path: &str) -> PathBuf {
            self.path.join(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn temp_dir(test_name: &str) -> TempDir {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("crbsh-{test_name}-{nanos}"));

        fs::create_dir(&dir).unwrap();

        TempDir { path: dir }
    }
}
