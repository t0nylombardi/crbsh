pub mod registry;

pub mod alias;
pub mod cd;
pub mod exit;
pub mod export;
pub mod fg;
pub mod history;
pub mod jobs;
pub mod print;
pub mod set;
pub mod unalias;
pub mod unset;

#[derive(Debug)]
pub enum BuiltinOutcome {
    Continue,
    ContinueWithStatus(i32),
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
