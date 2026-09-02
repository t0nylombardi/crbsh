mod command;
mod error;
mod jobs;
mod pipeline;
mod redirect;
mod render;
mod structured;

pub use error::ExecutionError;
pub use jobs::{JobError, JobId, JobManager, JobState};
pub use pipeline::{execute_background_pipeline, execute_pipeline};
pub(crate) use structured::SHELL_HOST_TYPES;
