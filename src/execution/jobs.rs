use std::process::Child;

use std::os::unix::process::ExitStatusExt;

pub type JobId = u32;

pub struct Job {
    pub id: JobId,
    pub command: String,
    children: Vec<Child>,
    state: JobState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done,
}

pub struct JobStatus {
    pub id: JobId,
    pub state: JobState,
    pub command: String,
}

pub struct JobManager {
    jobs: Vec<Job>,
    next_id: JobId,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JobError {
    NoActiveJob,
    NoSuchJob(JobId),
    JobNotRunning(JobId),
    WaitFailed(String),
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            next_id: 1,
        }
    }

    pub fn add(&mut self, command: impl Into<String>, children: Vec<Child>) -> JobId {
        let id = self.next_id;
        self.next_id += 1;

        self.jobs.push(Job {
            id,
            command: command.into(),
            children,
            state: JobState::Running,
        });

        id
    }

    pub fn statuses(&mut self) -> Vec<JobStatus> {
        self.refresh();

        self.jobs
            .iter()
            .map(|job| JobStatus {
                id: job.id,
                state: job.state,
                command: job.command.clone(),
            })
            .collect()
    }

    pub fn foreground(&mut self, requested_id: Option<JobId>) -> Result<i32, JobError> {
        self.refresh();

        let Some(index) = self.foreground_job_index(requested_id) else {
            return Err(match requested_id {
                Some(id) => JobError::NoSuchJob(id),
                None => JobError::NoActiveJob,
            });
        };

        if self.jobs[index].state != JobState::Running {
            return Err(JobError::JobNotRunning(self.jobs[index].id));
        }

        let mut job = self.jobs.remove(index);
        let mut exit_code = 0;

        for child in &mut job.children {
            let status = child
                .wait()
                .map_err(|err| JobError::WaitFailed(err.to_string()))?;
            exit_code = status_exit_code(status);
        }

        Ok(exit_code)
    }

    fn foreground_job_index(&self, requested_id: Option<JobId>) -> Option<usize> {
        match requested_id {
            Some(id) => self.jobs.iter().position(|job| job.id == id),
            None => self
                .jobs
                .iter()
                .rposition(|job| job.state == JobState::Running),
        }
    }

    fn refresh(&mut self) {
        for job in &mut self.jobs {
            if job.state == JobState::Done {
                continue;
            }

            let mut all_done = true;

            for child in &mut job.children {
                match child.try_wait() {
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => all_done = false,
                }
            }

            if all_done {
                job.state = JobState::Done;
            }
        }
    }
}

fn status_exit_code(status: std::process::ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

impl Drop for JobManager {
    fn drop(&mut self) {
        for job in &mut self.jobs {
            for child in &mut job.children {
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    use super::{JobManager, JobState};

    #[test]
    fn reports_running_and_done_jobs() {
        let mut manager = JobManager::new();
        let child = Command::new("sleep").arg("0.1").spawn().unwrap();

        let id = manager.add("sleep 0.1", vec![child]);

        let statuses = manager.statuses();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].id, id);
        assert_eq!(statuses[0].state, JobState::Running);

        thread::sleep(Duration::from_millis(150));

        let statuses = manager.statuses();
        assert_eq!(statuses[0].state, JobState::Done);
        assert_eq!(statuses[0].command, "sleep 0.1");
    }

    #[test]
    fn foreground_waits_for_requested_running_job_and_removes_it() {
        let mut manager = JobManager::new();
        let child = Command::new("sleep").arg("0.1").spawn().unwrap();

        let id = manager.add("sleep 0.1", vec![child]);

        assert_eq!(manager.foreground(Some(id)), Ok(0));
        assert!(manager.statuses().is_empty());
    }

    #[test]
    fn foreground_without_id_uses_most_recent_running_job() {
        let mut manager = JobManager::new();
        let first = Command::new("sleep").arg("0.1").spawn().unwrap();
        let second = Command::new("sleep").arg("0.1").spawn().unwrap();

        let first_id = manager.add("sleep 0.1", vec![first]);
        let second_id = manager.add("sleep 0.1", vec![second]);

        assert_eq!(manager.foreground(None), Ok(0));

        let statuses = manager.statuses();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].id, first_id);
        assert_ne!(statuses[0].id, second_id);
    }

    #[test]
    fn foreground_reports_missing_active_job() {
        let mut manager = JobManager::new();

        assert_eq!(manager.foreground(None), Err(super::JobError::NoActiveJob));
        assert_eq!(
            manager.foreground(Some(7)),
            Err(super::JobError::NoSuchJob(7))
        );
    }
}
