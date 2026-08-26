use crate::shell::ShellError;

#[derive(Debug)]
pub struct ExecutionError {
    pub command: String,
    pub message: String,
}

impl From<ShellError> for ExecutionError {
    fn from(error: ShellError) -> Self {
        Self {
            command: "evaluation".into(),
            message: error.to_string(),
        }
    }
}
