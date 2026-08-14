pub mod registry;

pub mod cd;
pub mod exit;
pub mod print;

#[derive(Debug)]
pub enum BuiltinOutcome {
    Continue,
    Exit(i32),
}

#[derive(Debug)]
pub struct BuiltinError {
    pub message: String,
}

impl BuiltinError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub type BuiltinResult = Result<BuiltinOutcome, BuiltinError>;
