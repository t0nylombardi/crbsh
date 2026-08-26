use std::collections::BTreeMap;

use crate::parser::{Expression, ParsedCommand, Pipeline};
use crate::runtime::Value;
use crate::shell::Shell;

use super::ExecutionError;

const STRUCTURED_COMMANDS: &[&str] = &["values", "record", "take", "count", "collect"];

pub(super) fn contains_structured_command(pipeline: &Pipeline) -> bool {
    pipeline
        .commands
        .iter()
        .any(|command| STRUCTURED_COMMANDS.contains(&command.name.as_str()))
}

pub(super) fn execute_native_pipeline(
    shell: &Shell,
    pipeline: &Pipeline,
) -> Result<Vec<Value>, ExecutionError> {
    let mut stream = None;

    for (index, command) in pipeline.commands.iter().enumerate() {
        if !STRUCTURED_COMMANDS.contains(&command.name.as_str()) {
            return Err(stage_error(
                index,
                command,
                "external command requires the structured pipeline adapter",
            ));
        }
        if !command.redirections.is_empty() {
            return Err(stage_error(
                index,
                command,
                "redirection requires the structured pipeline renderer",
            ));
        }

        stream = Some(execute_stage(shell, index, command, stream)?);
    }

    Ok(stream.unwrap_or_default())
}

fn execute_stage(
    shell: &Shell,
    index: usize,
    command: &ParsedCommand,
    input: Option<Vec<Value>>,
) -> Result<Vec<Value>, ExecutionError> {
    match command.name.as_str() {
        "values" => {
            require_no_input(index, command, &input)?;
            let values = evaluate_args(shell, index, command)?;
            Ok(values
                .into_iter()
                .flat_map(|value| match value {
                    Value::List(values) => values,
                    value => vec![value],
                })
                .collect())
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
            Ok(vec![Value::Record(fields)])
        }
        "take" => {
            let mut values = require_input(index, command, input)?;
            let count = single_non_negative_integer(shell, index, command)?;
            values.truncate(count);
            Ok(values)
        }
        "count" => {
            require_no_args(index, command)?;
            let values = require_input(index, command, input)?;
            let count = i64::try_from(values.len())
                .map_err(|_| stage_error(index, command, "stream length exceeds int range"))?;
            Ok(vec![Value::Int(count)])
        }
        "collect" => {
            require_no_args(index, command)?;
            Ok(vec![Value::List(require_input(index, command, input)?)])
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
    input: &Option<Vec<Value>>,
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
    input: Option<Vec<Value>>,
) -> Result<Vec<Value>, ExecutionError> {
    input.ok_or_else(|| stage_error(index, command, "consumer requires structured input"))
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
            execute_native_pipeline(&shell, &pipeline).unwrap(),
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
            execute_native_pipeline(&shell, &pipeline).unwrap(),
            vec![Value::Int(1)]
        );
    }

    #[test]
    fn validates_stage_types_and_arguments() {
        let shell = Shell::new();
        let parser::ParsedInput::Pipeline(pipeline) = parser::parse("take 1").unwrap() else {
            panic!("expected pipeline");
        };

        let error = execute_native_pipeline(&shell, &pipeline).unwrap_err();
        assert_eq!(error.command, "take");
        assert!(error.message.contains("consumer requires structured input"));
    }
}
