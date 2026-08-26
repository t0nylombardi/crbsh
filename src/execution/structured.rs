use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::parser::{Expression, ParsedCommand, Pipeline};
use crate::runtime::{Value, ValueStream};
use crate::shell::Shell;

use super::ExecutionError;
use super::command::resolved_args;
use super::pipeline::status_exit_code;

const STRUCTURED_COMMANDS: &[&str] = &["values", "record", "take", "count", "collect"];

pub(super) fn contains_structured_command(pipeline: &Pipeline) -> bool {
    pipeline
        .commands
        .iter()
        .any(|command| STRUCTURED_COMMANDS.contains(&command.name.as_str()))
}

#[derive(Debug)]
pub(super) enum PipelineData {
    Text(Vec<u8>),
    Structured(ValueStream),
}

#[derive(Debug)]
pub(super) struct StructuredPipelineOutput {
    pub data: PipelineData,
    pub exit_code: i32,
}

pub(super) fn execute_structured_pipeline(
    shell: &Shell,
    pipeline: &Pipeline,
) -> Result<StructuredPipelineOutput, ExecutionError> {
    let mut stream = None;
    let mut exit_code = 0;

    for (index, command) in pipeline.commands.iter().enumerate() {
        if command.redirections.stdin.is_some() {
            return Err(stage_error(
                index,
                command,
                "input redirection is not supported in structured pipelines",
            ));
        }
        if command.redirections.stdout.is_some() && index + 1 != pipeline.commands.len() {
            return Err(stage_error(
                index,
                command,
                "output redirection is only valid on the final stage",
            ));
        }

        if STRUCTURED_COMMANDS.contains(&command.name.as_str()) {
            stream = Some(execute_stage(shell, index, command, stream)?);
            exit_code = 0;
        } else {
            let output = execute_external_stage(shell, index, command, stream)?;
            stream = Some(PipelineData::Text(output.stdout));
            exit_code = output.exit_code;
        }
    }

    Ok(StructuredPipelineOutput {
        data: stream.unwrap_or_else(|| PipelineData::Structured(ValueStream::empty())),
        exit_code,
    })
}

fn execute_stage(
    shell: &Shell,
    index: usize,
    command: &ParsedCommand,
    input: Option<PipelineData>,
) -> Result<PipelineData, ExecutionError> {
    match command.name.as_str() {
        "values" => {
            require_no_input(index, command, &input)?;
            let values = evaluate_args(shell, index, command)?;
            Ok(PipelineData::Structured(ValueStream::from_values(values)))
        }
        "record" => {
            require_no_input(index, command, &input)?;
            if !command.args.len().is_multiple_of(2) {
                return Err(stage_error(
                    index,
                    command,
                    "expected key/value argument pairs",
                ));
            }

            let mut fields = BTreeMap::new();
            for pair in command.args.chunks_exact(2) {
                let key = record_key(&pair[0]).ok_or_else(|| {
                    stage_error(index, command, "record keys must be identifiers or strings")
                })?;
                let value = shell
                    .evaluate(&pair[1])
                    .map_err(|error| stage_error(index, command, error.to_string()))?;
                fields.insert(key, value);
            }
            Ok(PipelineData::Structured(ValueStream::from_record(fields)))
        }
        "take" => {
            let values = require_input(index, command, input)?;
            let count = single_non_negative_integer(shell, index, command)?;
            Ok(PipelineData::Structured(values.take(count)))
        }
        "count" => {
            require_no_args(index, command)?;
            let values = require_input(index, command, input)?;
            let values = values
                .count()
                .map_err(|_| stage_error(index, command, "stream length exceeds int range"))?;
            Ok(PipelineData::Structured(values))
        }
        "collect" => {
            require_no_args(index, command)?;
            Ok(PipelineData::Structured(
                require_input(index, command, input)?.collect(),
            ))
        }
        _ => unreachable!("structured command checked by caller"),
    }
}

fn evaluate_args(
    shell: &Shell,
    index: usize,
    command: &ParsedCommand,
) -> Result<Vec<Value>, ExecutionError> {
    command
        .args
        .iter()
        .map(|argument| {
            shell
                .evaluate(argument)
                .map_err(|error| stage_error(index, command, error.to_string()))
        })
        .collect()
}

fn record_key(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Identifier(value) | Expression::Literal(Value::String(value)) => {
            Some(value.clone())
        }
        _ => None,
    }
}

fn single_non_negative_integer(
    shell: &Shell,
    index: usize,
    command: &ParsedCommand,
) -> Result<usize, ExecutionError> {
    let [argument] = command.args.as_slice() else {
        return Err(stage_error(index, command, "expected one integer argument"));
    };
    let value = shell
        .evaluate(argument)
        .map_err(|error| stage_error(index, command, error.to_string()))?;
    let Value::Int(value) = value else {
        return Err(stage_error(index, command, "expected an integer argument"));
    };
    usize::try_from(value)
        .map_err(|_| stage_error(index, command, "expected a non-negative integer argument"))
}

fn require_no_input(
    index: usize,
    command: &ParsedCommand,
    input: &Option<PipelineData>,
) -> Result<(), ExecutionError> {
    if input.is_some() {
        return Err(stage_error(
            index,
            command,
            "producer must be the first stage",
        ));
    }
    Ok(())
}

fn require_input(
    index: usize,
    command: &ParsedCommand,
    input: Option<PipelineData>,
) -> Result<ValueStream, ExecutionError> {
    match input {
        Some(PipelineData::Structured(values)) => Ok(values),
        Some(PipelineData::Text(bytes)) => text_to_values(index, command, bytes),
        None => Err(stage_error(
            index,
            command,
            "consumer requires structured input",
        )),
    }
}

struct ExternalOutput {
    stdout: Vec<u8>,
    exit_code: i32,
}

fn execute_external_stage(
    shell: &Shell,
    index: usize,
    command: &ParsedCommand,
    input: Option<PipelineData>,
) -> Result<ExternalOutput, ExecutionError> {
    if shell.builtins.get(&command.name).is_some() {
        return Err(stage_error(
            index,
            command,
            "stateful builtins cannot consume structured pipelines",
        ));
    }

    let mut process = Command::new(&command.name);
    process
        .args(resolved_args(shell, command)?)
        .envs(shell.environment_overrides())
        .stdout(Stdio::piped());

    let input = input.map(pipeline_data_to_text);
    if input.is_some() {
        process.stdin(Stdio::piped());
    }

    let mut child = process
        .spawn()
        .map_err(|error| stage_error(index, command, error.to_string()))?;
    let writer = input.and_then(|input| {
        child
            .stdin
            .take()
            .map(|mut stdin| std::thread::spawn(move || stdin.write_all(&input)))
    });
    let output = child
        .wait_with_output()
        .map_err(|error| stage_error(index, command, error.to_string()))?;

    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| stage_error(index, command, "stdin adapter thread panicked"))?
            .map_err(|error| stage_error(index, command, error.to_string()))?;
    }

    Ok(ExternalOutput {
        stdout: output.stdout,
        exit_code: status_exit_code(output.status),
    })
}

fn pipeline_data_to_text(data: PipelineData) -> Vec<u8> {
    match data {
        PipelineData::Text(bytes) => bytes,
        PipelineData::Structured(values) => {
            let mut output = values
                .into_values()
                .into_iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes();
            if !output.is_empty() {
                output.push(b'\n');
            }
            output
        }
    }
}

fn text_to_values(
    index: usize,
    command: &ParsedCommand,
    bytes: Vec<u8>,
) -> Result<ValueStream, ExecutionError> {
    let text = String::from_utf8(bytes)
        .map_err(|_| stage_error(index, command, "external output is not valid UTF-8"))?;
    Ok(ValueStream::from_text_lines(
        text.lines()
            .map(|line| Value::String(line.to_string()))
            .collect(),
    ))
}

fn require_no_args(index: usize, command: &ParsedCommand) -> Result<(), ExecutionError> {
    if !command.args.is_empty() {
        return Err(stage_error(index, command, "expected no arguments"));
    }
    Ok(())
}

fn stage_error(
    index: usize,
    command: &ParsedCommand,
    message: impl Into<String>,
) -> ExecutionError {
    ExecutionError {
        command: command.name.clone(),
        message: format!(
            "structured pipeline stage {}: {}",
            index + 1,
            message.into()
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::parser;

    use super::*;

    #[test]
    fn lists_stream_as_items_and_collect_rebuilds_the_list() {
        let shell = Shell::new();
        let parser::ParsedInput::Pipeline(pipeline) =
            parser::parse("values [1, 2, 3] | take 2 | collect").unwrap()
        else {
            panic!("expected pipeline");
        };

        assert_eq!(
            structured_values(execute_structured_pipeline(&shell, &pipeline).unwrap()),
            vec![Value::List(vec![Value::Int(1), Value::Int(2)])]
        );
    }

    #[test]
    fn records_are_atomic_stream_items() {
        let shell = Shell::new();
        let parser::ParsedInput::Pipeline(pipeline) =
            parser::parse(r#"record name "Tony" active true | count"#).unwrap()
        else {
            panic!("expected pipeline");
        };

        assert_eq!(
            structured_values(execute_structured_pipeline(&shell, &pipeline).unwrap()),
            vec![Value::Int(1)]
        );
    }

    #[test]
    fn validates_stage_types_and_arguments() {
        let shell = Shell::new();
        let parser::ParsedInput::Pipeline(pipeline) = parser::parse("take 1").unwrap() else {
            panic!("expected pipeline");
        };

        let error = execute_structured_pipeline(&shell, &pipeline).unwrap_err();
        assert_eq!(error.command, "take");
        assert!(error.message.contains("consumer requires structured input"));
    }

    #[test]
    fn adapts_structured_values_through_external_commands() {
        let shell = Shell::new();
        let parser::ParsedInput::Pipeline(pipeline) =
            parser::parse("values [\"crab\", \"shell\"] | grep crab | collect").unwrap()
        else {
            panic!("expected pipeline");
        };

        assert_eq!(
            structured_values(execute_structured_pipeline(&shell, &pipeline).unwrap()),
            vec![Value::List(vec![Value::String("crab".into())])]
        );
    }

    #[test]
    fn adapts_external_text_lines_for_structured_consumers() {
        let shell = Shell::new();
        let parser::ParsedInput::Pipeline(pipeline) =
            parser::parse("printf \"first\nsecond\n\" | count").unwrap()
        else {
            panic!("expected pipeline");
        };

        assert_eq!(
            structured_values(execute_structured_pipeline(&shell, &pipeline).unwrap()),
            vec![Value::Int(2)]
        );
    }

    fn structured_values(output: StructuredPipelineOutput) -> Vec<Value> {
        let PipelineData::Structured(values) = output.data else {
            panic!("expected structured output");
        };
        values.into_values()
    }
}
