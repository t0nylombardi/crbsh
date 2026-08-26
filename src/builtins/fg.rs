use crate::execution::{JobError, JobId};
use crate::shell::Shell;

use super::{BuiltinError, BuiltinOutcome, BuiltinResult};

pub fn run(shell: &mut Shell, args: &[String]) -> BuiltinResult {
    if args.len() > 1 {
        return Err(BuiltinError::new("fg: expected at most one job id"));
    }

    let requested_id = match args.first() {
        Some(value) => Some(
            value
                .parse::<JobId>()
                .map_err(|_| BuiltinError::new("fg: expected numeric job id"))?,
        ),
        None => None,
    };

    shell
        .jobs
        .foreground(requested_id)
        .map(BuiltinOutcome::ContinueWithStatus)
        .map_err(|err| BuiltinError::new(fg_error(err)))
}

fn fg_error(error: JobError) -> String {
    match error {
        JobError::NoActiveJob => "fg: no active job".into(),
        JobError::NoSuchJob(id) => format!("fg: job {id} not found"),
        JobError::JobNotRunning(id) => format!("fg: job {id} is not running"),
        JobError::WaitFailed(message) => format!("fg: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::super::BuiltinOutcome;
    use super::run;
    use crate::shell::Shell;

    #[test]
    fn foregrounds_requested_job() {
        let mut shell = Shell::new();
        let child = Command::new("sleep").arg("0.1").spawn().unwrap();
        let id = shell.jobs.add("sleep 0.1", vec![child]);

        let result = run(&mut shell, &[id.to_string()]).unwrap();

        assert!(matches!(result, BuiltinOutcome::ContinueWithStatus(0)));
        assert!(shell.jobs.statuses().is_empty());
    }

    #[test]
    fn rejects_invalid_job_id() {
        let mut shell = Shell::new();
        let error = run(&mut shell, &["latest".into()]).unwrap_err();

        assert_eq!(error.message, "fg: expected numeric job id");
    }
}
