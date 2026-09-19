use std::collections::BinaryHeap;
use std::cmp::Ordering;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use anyhow::Result;

/// A single polling job with its schedule and action
pub struct PollJob {
    /// Unique identifier for this job
    pub id: String,
    /// Next time this job should run
    pub next_run: Instant,
    /// Interval between runs
    pub interval: Duration,
    /// The actual polling function
    pub poll_fn: Arc<dyn Fn() -> Result<()> + Send + Sync>,
}

impl PollJob {
    pub fn new<F>(id: String, interval: Duration, poll_fn: F) -> Self
    where
        F: Fn() -> Result<()> + Send + Sync + 'static,
    {
        Self {
            id,
            next_run: Instant::now(),
            interval,
            poll_fn: Arc::new(poll_fn),
        }
    }

    /// Execute the poll function and update next_run
    pub fn execute(&mut self) -> Result<()> {
        let result = (self.poll_fn)();
        // Always reschedule, even on error
        self.next_run = Instant::now() + self.interval;
        result
    }
}

// Min-heap by next_run: BinaryHeap is a max-heap, so Ord is reversed to put
// the EARLIEST next_run at the top. Ord/PartialOrd/Eq/PartialEq must agree
// (the Rust contract a == b => cmp == Equal); the old PartialEq keyed on id
// alone and broke it, which BinaryHeap relies on.
impl Ord for PollJob {
    fn cmp(&self, other: &Self) -> Ordering {
        other.next_run.cmp(&self.next_run)
    }
}

impl PartialOrd for PollJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        other.next_run.partial_cmp(&self.next_run)
    }
}

impl Eq for PollJob {}

impl PartialEq for PollJob {
    fn eq(&self, other: &Self) -> bool {
        self.next_run == other.next_run
    }
}

/// Commands that can be sent to the scheduler
pub enum SchedulerCommand {
    /// Add a new polling job
    AddJob(PollJob),
    /// Update the interval of an existing job
    UpdateInterval(String, Duration),
    /// Remove a job by ID
    RemoveJob(String),
    /// Shutdown the scheduler
    Shutdown,
}

/// The main polling scheduler that manages all polling jobs
pub struct PollingScheduler {
    jobs: Arc<RwLock<BinaryHeap<PollJob>>>,
    command_rx: mpsc::UnboundedReceiver<SchedulerCommand>,
    command_tx: mpsc::UnboundedSender<SchedulerCommand>,
}

impl PollingScheduler {
    /// Create a new polling scheduler
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        
        Self {
            jobs: Arc::new(RwLock::new(BinaryHeap::new())),
            command_rx,
            command_tx,
        }
    }

    /// Get a handle to send commands to the scheduler
    pub fn get_handle(&self) -> SchedulerHandle {
        SchedulerHandle {
            command_tx: self.command_tx.clone(),
        }
    }

    /// Run the scheduler loop
    pub async fn run(mut self) {
        log::info!("Starting polling scheduler");
        
        loop {
            // Calculate sleep duration until next job
            let sleep_duration = {
                let jobs = self.jobs.read().unwrap();
                if let Some(next_job) = jobs.peek() {
                    let now = Instant::now();
                    if next_job.next_run > now {
                        next_job.next_run - now
                    } else {
                        Duration::from_millis(0)
                    }
                } else {
                    // No jobs, sleep for a while
                    Duration::from_secs(1)
                }
            };

            // Wait for either timeout or command
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    // Time to execute job(s)
                    self.execute_due_jobs();
                }
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        SchedulerCommand::AddJob(job) => {
                            log::debug!("Adding poll job: {}", job.id);
                            self.add_or_replace_job(job);
                        }
                        SchedulerCommand::UpdateInterval(id, interval) => {
                            log::debug!("Updating poll interval for {}: {:?}", id, interval);
                            self.update_job_interval(&id, interval);
                        }
                        SchedulerCommand::RemoveJob(id) => {
                            log::debug!("Removing poll job: {}", id);
                            self.remove_job(&id);
                        }
                        SchedulerCommand::Shutdown => {
                            log::info!("Shutting down polling scheduler");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Execute all jobs that are due
    fn execute_due_jobs(&self) {
        let now = Instant::now();
        let mut jobs = self.jobs.write().unwrap();
        let mut due_jobs = Vec::new();

        // Collect all due jobs
        while let Some(job) = jobs.peek() {
            if job.next_run <= now {
                due_jobs.push(jobs.pop().unwrap());
            } else {
                break;
            }
        }

        // Execute jobs and re-insert them
        for mut job in due_jobs {
            match job.execute() {
                Ok(_) => {
                    log::trace!("Executed poll job: {}", job.id);
                }
                Err(e) => {
                    log::error!("Error executing poll job {}: {}", job.id, e);
                    // Reschedule even on error (execute() already updated next_run)
                }
            }
            jobs.push(job);
        }
    }

    /// Update the interval of a job
    fn update_job_interval(&self, id: &str, new_interval: Duration) {
        let mut jobs = self.jobs.write().unwrap();
        let mut temp_jobs: Vec<PollJob> = jobs.drain().collect();
        
        for job in &mut temp_jobs {
            if job.id == id {
                job.interval = new_interval;
                // Reset next_run to reflect new interval
                job.next_run = Instant::now() + new_interval;
                log::info!("Updated interval for job {} to {:?}", id, new_interval);
            }
        }

        for job in temp_jobs {
            jobs.push(job);
        }
    }

    /// Add a job, replacing any existing job with the same id. A duplicate id
    /// in the heap would make `remove_job` act on both copies and
    /// `update_job_interval` double the poll work; BinaryHeap has no in-place
    /// replace that preserves the heap invariant, so drain-filter-push like
    /// `remove_job`.
    fn add_or_replace_job(&self, job: PollJob) {
        let mut jobs = self.jobs.write().unwrap();
        let id = job.id.clone();
        let before = jobs.len();
        let mut kept: Vec<PollJob> = jobs.drain().filter(|j| j.id != id).collect();
        if kept.len() != before {
            log::warn!("Replaced existing poll job: {id}");
        }
        kept.push(job);
        for j in kept {
            jobs.push(j);
        }
    }

    /// Remove a job by ID
    fn remove_job(&self, id: &str) {
        let mut jobs = self.jobs.write().unwrap();
        let temp_jobs: Vec<PollJob> = jobs.drain().filter(|j| j.id != id).collect();
        
        for job in temp_jobs {
            jobs.push(job);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BinaryHeap;
    use std::time::{Duration, Instant};

    fn job(id: String, next_run: Instant) -> PollJob {
        PollJob {
            id,
            next_run,
            interval: Duration::from_secs(1),
            poll_fn: Arc::new(|| Ok(())),
        }
    }

    /// Two jobs with the same id but different next_run: the OLD impls broke
    /// the Ord/Eq contract (PartialEq said equal, Ord said not), which
    /// BinaryHeap relies on. The fixed impls key all four traits on
    /// next_run, so the contract holds.
    #[test]
    fn ord_eq_contract_holds() {
        let now = Instant::now();
        let a = job("x".to_string(), now + Duration::from_secs(1));
        let b = job("x".to_string(), now + Duration::from_secs(5));
        // contract: a == b  <=>  cmp(a,b) == Equal
        assert_eq!(a == b, a.cmp(&b) == Ordering::Equal);
        assert!(a != b);
        // Ord is REVERSED for the min-heap, so the earlier job orders Greater
        assert_eq!(a.cmp(&b), Ordering::Greater);
        assert_eq!(b.cmp(&a), Ordering::Less);
    }

    #[test]
    fn binary_heap_is_a_min_heap_by_next_run() {
        let now = Instant::now();
        let mut heap: BinaryHeap<PollJob> = BinaryHeap::new();
        heap.push(job("a".to_string(), now + Duration::from_secs(3)));
        heap.push(job("b".to_string(), now + Duration::from_secs(1)));
        heap.push(job("c".to_string(), now + Duration::from_secs(2)));
        // earliest next_run must come out FIRST (min-heap, as the original
        // reversed Ord intended)
        let top = heap.pop().unwrap();
        assert_eq!(top.id, "b");
        assert_eq!(heap.pop().unwrap().id, "c");
        assert_eq!(heap.pop().unwrap().id, "a");
    }

    #[test]
    fn add_job_with_duplicate_id_replaces_not_doubles() {
        // Drives the real AddJob code path (add_or_replace_job, the method
        // the scheduler loop calls) -- no async runtime is available in a
        // sync test, so the command channel is not used.
        let sched = PollingScheduler::new();
        let closure: fn() -> Result<()> = || Ok(());

        // First add: empty heap -> 1 job.
        let mut first = PollJob::new("hw".to_string(), Duration::from_secs(1), closure);
        let _ = first.execute();
        sched.add_or_replace_job(first);
        assert_eq!(sched.jobs.read().unwrap().len(), 1);

        // Second add with the SAME id: must REPLACE, not append -> still 1.
        let mut second = PollJob::new("hw".to_string(), Duration::from_secs(5), closure);
        let _ = second.execute();
        sched.add_or_replace_job(second);

        let jobs = sched.jobs.read().unwrap();
        assert_eq!(jobs.len(), 1, "duplicate id must replace, not append");
        let surviving = jobs.iter().find(|j| j.id == "hw").unwrap();
        assert_eq!(surviving.interval, Duration::from_secs(5));

        // A different id is unaffected.
        let mut third = PollJob::new("fan".to_string(), Duration::from_secs(2), closure);
        let _ = third.execute();
        drop(jobs);
        sched.add_or_replace_job(third);
        assert_eq!(sched.jobs.read().unwrap().len(), 2);
    }
}

/// Handle to interact with the scheduler
#[derive(Clone)]
pub struct SchedulerHandle {
    command_tx: mpsc::UnboundedSender<SchedulerCommand>,
}

impl SchedulerHandle {
    /// Add a new polling job
    pub fn add_job(&self, job: PollJob) -> Result<()> {
        self.command_tx
            .send(SchedulerCommand::AddJob(job))
            .map_err(|e| anyhow::anyhow!("Failed to add job: {}", e))
    }

    /// Update the interval of an existing job
    pub fn update_interval(&self, id: String, interval: Duration) -> Result<()> {
        self.command_tx
            .send(SchedulerCommand::UpdateInterval(id, interval))
            .map_err(|e| anyhow::anyhow!("Failed to update interval: {}", e))
    }

    /// Remove a job
    pub fn remove_job(&self, id: String) -> Result<()> {
        self.command_tx
            .send(SchedulerCommand::RemoveJob(id))
            .map_err(|e| anyhow::anyhow!("Failed to remove job: {}", e))
    }

    /// Shutdown the scheduler
    pub fn shutdown(&self) -> Result<()> {
        self.command_tx
            .send(SchedulerCommand::Shutdown)
            .map_err(|e| anyhow::anyhow!("Failed to shutdown: {}", e))
    }
}
