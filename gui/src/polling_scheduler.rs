use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use anyhow::Result;

// ---------------------------------------------------------------------------
// One outstanding request per component
// ---------------------------------------------------------------------------
//
// The coordinator fires a tick on a timer and hands the component id to the
// callback, which spawns a D-Bus fetch. Nothing tied the fetch's lifetime to
// the next tick, so when the consumer stalls — a hidden/iconified window stops
// calling `ui()` and `logic()`, and therefore stops draining the update
// channel — the fetch tasks pile up: each one parked on `Sender::send()` while
// the coordinator keeps starting more. The queue is bounded (100 slots), so the
// growth does not stop at 100: it moves into the task set, one blocked task per
// tick, for as long as the window stays hidden.
//
// `InFlightSet` bounds that instead. A component with a request still in flight
// is skipped for this tick (counted, not queued), and the permit is released
// when the fetch task finishes — including on the paths where the fetch returns
// nothing to send.

/// Components with an outstanding request, plus a count of skipped ticks.
#[derive(Default)]
pub struct InFlightSet {
    active: Mutex<HashMap<String, u32>>,
    skipped: AtomicU64,
}

impl InFlightSet {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Claim `id` for a new request, or `None` when one is already in flight.
    ///
    /// A refused claim is a skipped tick, not a queued one: the caller simply
    /// does not spawn anything this round.
    pub fn try_begin(self: &Arc<Self>, id: &str) -> Option<InFlightPermit> {
        let mut active = self.lock();
        let entry = active.entry(id.to_string()).or_insert(0);
        if *entry > 0 {
            drop(active);
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        *entry = 1;
        Some(InFlightPermit {
            set: Arc::clone(self),
            id: id.to_string(),
        })
    }

    /// Number of ticks skipped because a request was still in flight.
    pub fn skipped_ticks(&self) -> u64 {
        self.skipped.load(Ordering::Relaxed)
    }

    /// How many components currently have a request in flight.
    pub fn active_len(&self) -> usize {
        self.lock().values().filter(|n| **n > 0).count()
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.lock().get(id).copied().unwrap_or(0) > 0
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, u32>> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Releases its component's claim when dropped, so the claim cannot outlive
/// the fetch task even on an early return or a panic.
pub struct InFlightPermit {
    set: Arc<InFlightSet>,
    id: String,
}

impl Drop for InFlightPermit {
    fn drop(&mut self) {
        let mut active = self.set.lock();
        if let Some(entry) = active.get_mut(&self.id) {
            *entry = 0;
        }
    }
}

#[cfg(test)]
mod in_flight_tests {
    use super::*;

    #[test]
    fn a_second_claim_is_refused_while_the_first_is_held() {
        let set = InFlightSet::new();
        let first = set.try_begin("cpu").expect("first claim must succeed");

        assert!(set.is_active("cpu"));
        assert!(set.try_begin("cpu").is_none(), "second claim must be refused");
        assert_eq!(set.skipped_ticks(), 1);
        assert_eq!(set.active_len(), 1);

        drop(first);
        assert!(!set.is_active("cpu"));
        assert!(set.try_begin("cpu").is_some(), "claim must be free again");
        assert_eq!(set.skipped_ticks(), 1, "a refusal is the only skip");
    }

    #[test]
    fn claims_are_per_component() {
        let set = InFlightSet::new();
        let _cpu = set.try_begin("cpu").expect("cpu");
        let _gpu = set.try_begin("gpu").expect("gpu");

        assert!(set.try_begin("cpu").is_none());
        assert!(set.try_begin("gpu").is_none());
        assert_eq!(set.active_len(), 2);
        assert_eq!(set.skipped_ticks(), 2);
    }

    #[test]
    fn a_stalled_consumer_bounds_the_task_set_to_one_per_component() {
        // The iconify scenario: the consumer never drains, and the coordinator
        // keeps ticking. Task count must stay at the number of components.
        let set = InFlightSet::new();
        let components = ["cpu", "gpu", "memory", "fans", "battery"];
        let mut permits = Vec::new();

        for _tick in 0..500 {
            for id in components {
                if let Some(permit) = set.try_begin(id) {
                    // A tick that claims the slot models the spawned fetch;
                    // the fetch that never completes keeps its permit.
                    if permits.iter().any(|p: &InFlightPermit| p.id == id) {
                        drop(permit);
                    } else {
                        permits.push(permit);
                    }
                }
            }
        }

        assert_eq!(set.active_len(), components.len());
        assert_eq!(set.skipped_ticks(), 500 * components.len() as u64 - components.len() as u64);
    }

    #[test]
    fn the_permit_survives_being_moved_into_a_task() {
        let set = InFlightSet::new();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");

        let set_in_task = Arc::clone(&set);
        let set_outer = Arc::clone(&set);
        rt.block_on(async move {
            let handle = tokio::spawn(async move {
                let _permit = set_in_task.try_begin("gpu").expect("claim in task");
                assert!(set_outer.is_active("gpu"), "held while the fetch runs");
            });
            handle.await.expect("task");
        });

        assert!(!set.is_active("gpu"), "released when the task ended");
        assert!(set.try_begin("gpu").is_some());
    }
}

/// Lightweight UI refresh coordinator - manages when to trigger UI updates
/// Unlike a full scheduler, this just tracks intervals and notifies when refresh is needed
pub struct RefreshCoordinator {
    components: HashMap<String, ComponentRefresh>,
    command_rx: mpsc::UnboundedReceiver<CoordinatorCommand>,
    command_tx: mpsc::UnboundedSender<CoordinatorCommand>,
}

/// Tracks refresh timing for a single component
struct ComponentRefresh {
    interval: Duration,
    last_refresh: Option<Instant>,
}

impl ComponentRefresh {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_refresh: None, // None = never refreshed, needs immediate refresh
        }
    }

    fn should_refresh(&self) -> bool {
        match self.last_refresh {
            None => true, // First refresh is immediate
            Some(last) => last.elapsed() >= self.interval,
        }
    }

    fn mark_refreshed(&mut self) {
        self.last_refresh = Some(Instant::now());
    }

    fn time_until_refresh(&self) -> Duration {
        match self.last_refresh {
            None => Duration::from_millis(0), // Immediate refresh needed
            Some(last) => {
                let elapsed = last.elapsed();
                if elapsed >= self.interval {
                    Duration::from_millis(0)
                } else {
                    self.interval - elapsed
                }
            }
        }
    }
}

/// Commands for the coordinator
pub enum CoordinatorCommand {
    /// Register a component with its refresh interval
    Register(String, Duration),
    /// Update component refresh interval
    UpdateInterval(String, Duration),
}

impl RefreshCoordinator {
    /// Create a new refresh coordinator
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        
        Self {
            components: HashMap::new(),
            command_rx,
            command_tx,
        }
    }

    /// Get a handle to send commands
    pub fn get_handle(&self) -> CoordinatorHandle {
        CoordinatorHandle {
            command_tx: self.command_tx.clone(),
        }
    }

    /// Run the coordinator loop
    pub async fn run(mut self, refresh_callback: impl Fn(&str) + Send + 'static) {
        log::debug!("Starting UI refresh coordinator");
        
        loop {
            // Find next refresh time
            let sleep_duration = self.components
                .values()
                .map(|c| c.time_until_refresh())
                .min()
                .unwrap_or(Duration::from_millis(100));

            // Wait for either timeout or command
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    // Check which components need refresh
                    for (id, component) in self.components.iter_mut() {
                        if component.should_refresh() {
                            refresh_callback(id);
                            component.mark_refreshed();
                        }
                    }
                }
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        CoordinatorCommand::Register(id, interval) => {
                            log::debug!("Registering component: {} with interval {:?}", id, interval);
                            self.components.insert(id, ComponentRefresh::new(interval));
                        }
                        CoordinatorCommand::UpdateInterval(id, interval) => {
                            log::debug!("Updating interval for {}: {:?}", id, interval);
                            if let Some(component) = self.components.get_mut(&id) {
                                component.interval = interval;
                                log::info!("Updated interval for {} to {:?}", id, interval);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Handle to interact with the coordinator
#[derive(Clone)]
pub struct CoordinatorHandle {
    command_tx: mpsc::UnboundedSender<CoordinatorCommand>,
}

impl CoordinatorHandle {
    /// Register a component for refresh coordination
    pub fn register(&self, id: String, interval: Duration) -> Result<()> {
        self.command_tx
            .send(CoordinatorCommand::Register(id, interval))
            .map_err(|e| anyhow::anyhow!("Failed to register: {}", e))
    }

    /// Update the refresh interval for a component
    pub fn update_interval(&self, id: String, interval: Duration) -> Result<()> {
        self.command_tx
            .send(CoordinatorCommand::UpdateInterval(id, interval))
            .map_err(|e| anyhow::anyhow!("Failed to update interval: {}", e))
    }
}
