use crate::execution::JobState;
use crate::shell::Shell;

use super::{BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, _args: &[String]) -> BuiltinResult {
    for status in shell.jobs.statuses() {
        let label = match status.state {
            JobState::Running => "Running",
            JobState::Done => "Done",
        };

        println!("[{}] {:<8} {}", status.id, label, status.command);
    }

    Ok(BuiltinOutcome::Continue)
}
