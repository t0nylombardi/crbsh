use std::process::Child;

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
}
