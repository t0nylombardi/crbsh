use std::io::{self, Write};

use crate::parser::ParsedCommand;

use super::ExecutionError;
use super::redirect::output_file;
use super::structured::PipelineData;

pub(super) fn render_pipeline_output(
    command: &ParsedCommand,
    data: PipelineData,
) -> Result<(), ExecutionError> {
    let bytes = render(data);

    if let Some(redirection) = command.redirections.stdout.as_ref() {
        let mut file = output_file(command, &redirection.target, redirection.append)?;
        return file.write_all(&bytes).map_err(|error| ExecutionError {
            command: command.name.clone(),
            message: error.to_string(),
        });
    }

    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| ExecutionError {
            command: command.name.clone(),
            message: error.to_string(),
        })
}

fn render(data: PipelineData) -> Vec<u8> {
    match data {
        PipelineData::Text(bytes) => bytes,
        PipelineData::Structured(values) => {
            let mut bytes = values
                .into_iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes();
            if !bytes.is_empty() {
                bytes.push(b'\n');
            }
            bytes
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::runtime::Value;

    use super::*;

    #[test]
    fn renders_structured_values_one_item_per_line() {
        let mut record = BTreeMap::new();
        record.insert("name".into(), Value::String("Tony".into()));

        assert_eq!(
            render(PipelineData::Structured(vec![
                Value::Int(7),
                Value::Record(record),
                Value::List(vec![Value::Bool(true), Value::Bool(false)]),
            ])),
            b"7\n{name: Tony}\n[true, false]\n"
        );
    }

    #[test]
    fn preserves_external_text_exactly() {
        assert_eq!(
            render(PipelineData::Text(b"raw text".to_vec())),
            b"raw text"
        );
    }
}
