use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

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
        assert!(
            set.try_begin("cpu").is_none(),
            "second claim must be refused"
        );
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
        assert_eq!(
            set.skipped_ticks(),
            500 * components.len() as u64 - components.len() as u64
        );
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

#[cfg(test)]
mod pause_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    const COMPONENTS: [&str; 4] = ["cpu", "gpu", "memory", "logs"];

    /// Poll count for one component, 0 when it was never handed out.
    fn count(counts: &Counts, id: &str) -> usize {
        counts.lock().unwrap().get(id).copied().unwrap_or(0)
    }

    fn snapshot(counts: &Counts) -> BTreeMap<String, usize> {
        counts.lock().unwrap().clone()
    }

    /// Per-component poll counts.
    type Counts = Arc<Mutex<BTreeMap<String, usize>>>;

    /// Run a real coordinator for `settle_ms`, then narrow the poll set to
    /// `poll_set` and count polls per component over a further window.
    ///
    /// Counting rather than indexing the sample order: the coordinator hands
    /// components out in HashMap iteration order, so "the samples after the Nth
    /// cpu" is not a meaningful window. Two counting windows are.
    ///
    /// Deliberately NOT an `async fn`: it owns a runtime with timers enabled, so
    /// calling it from inside `#[tokio::test]` would try to block on a runtime
    /// from within one.
    fn counts_with_poll_set(poll_set: &[&str], settle_ms: u64) -> (Counts, Counts) {
        let before: Counts = Arc::new(Mutex::new(BTreeMap::new()));
        let after: Counts = Arc::new(Mutex::new(BTreeMap::new()));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");

        // Cloned out before the `async move` block, which takes the originals.
        let before_out = Arc::clone(&before);
        let after_out = Arc::clone(&after);

        runtime.block_on(async move {
            let coordinator = RefreshCoordinator::new();
            let handle = coordinator.get_handle();
            for id in COMPONENTS {
                handle
                    .register(id.to_string(), Duration::from_millis(10))
                    .expect("register");
            }

            let count_into = Arc::clone(&before);
            let switched = Arc::new(AtomicUsize::new(0));
            let switched_for_cb = Arc::clone(&switched);
            let after_for_cb = Arc::clone(&after);

            let runner = tokio::spawn(coordinator.run(move |id| {
                let target = if switched_for_cb.load(Ordering::SeqCst) == 0 {
                    Arc::clone(&count_into)
                } else {
                    Arc::clone(&after_for_cb)
                };
                *target.lock().unwrap().entry(id.to_string()).or_insert(0) += 1;
            }));

            // Let everything register and poll.
            tokio::time::sleep(Duration::from_millis(settle_ms)).await;

            handle
                .set_poll_set(poll_set.iter().map(|s| s.to_string()).collect())
                .expect("set poll set");
            switched.store(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(settle_ms)).await;

            runner.abort();
        });

        (before_out, after_out)
    }

    #[test]
    fn a_paused_component_is_never_refreshed() {
        let mut component = ComponentRefresh::new(Duration::from_millis(10));
        assert!(component.should_refresh(), "first refresh is immediate");

        component.paused = true;
        assert!(
            !component.should_refresh(),
            "a paused component never fires"
        );
    }

    #[test]
    fn a_paused_component_does_not_drag_the_sleep_to_zero() {
        // If a paused component reported "due now" the coordinator loop would
        // spin on it forever, burning CPU with the window hidden.
        let mut component = ComponentRefresh::new(Duration::from_millis(1));
        component.paused = true;
        assert!(
            component.time_until_refresh() > Duration::from_secs(60),
            "paused must not report an imminent refresh"
        );
    }

    #[test]
    fn resuming_does_not_fire_a_burst() {
        // last_refresh is kept while paused, so a component that was refreshed
        // before the pause does not become "never refreshed" and fire at once.
        let mut component = ComponentRefresh::new(Duration::from_secs(3600));
        component.mark_refreshed();
        component.paused = true;
        component.paused = false;
        assert!(
            !component.should_refresh(),
            "resuming must wait for the interval, not refresh immediately"
        );
    }

    #[test]
    fn only_the_named_components_keep_polling() {
        let (before, after) = counts_with_poll_set(&["cpu", "gpu"], 150);

        for id in ["cpu", "gpu"] {
            assert!(
                count(&after, id) > 0,
                "{id} is in the poll set and must keep polling: {:?}",
                snapshot(&after)
            );
            assert!(
                count(&before, id) > 0,
                "{id} must have been polling before the switch too"
            );
        }

        for id in ["memory", "logs"] {
            assert_eq!(
                count(&after, id),
                0,
                "{id} is not in the poll set and must not be polled: {:?}",
                snapshot(&after)
            );
            assert!(
                count(&before, id) > 0,
                "{id} must have been polling before the switch, or this proves nothing"
            );
        }
    }

    #[test]
    fn logs_is_never_polled_in_a_panel_poll_set() {
        // The ring reply is ~735 kB per call and no panel element reads it.
        let (_, after) = counts_with_poll_set(&["cpu", "memory"], 150);
        assert_eq!(
            count(&after, "logs"),
            0,
            "logs must never be in a panel poll set: {:?}",
            snapshot(&after)
        );
    }

    #[test]
    fn an_empty_poll_set_pauses_everything() {
        let (before, after) = counts_with_poll_set(&[], 150);
        assert!(
            snapshot(&before).values().any(|polls| *polls > 0),
            "everything was polling before the switch, or this proves nothing"
        );
        for (id, polls) in snapshot(&after) {
            assert_eq!(polls, 0, "{id} must not be polled with an empty set");
        }
    }

    #[test]
    fn restoring_the_full_set_resumes_polling() {
        // Panel -> normal is the same command with every component named; the
        // coordinator must not need to remember the previous poll set.
        let counts: Counts = Arc::new(Mutex::new(BTreeMap::new()));
        let switched = Arc::new(AtomicUsize::new(0));
        let switched_for_cb = Arc::clone(&switched);
        let counts_for_cb = Arc::clone(&counts);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            let coordinator = RefreshCoordinator::new();
            let handle = coordinator.get_handle();
            for id in COMPONENTS {
                handle
                    .register(id.to_string(), Duration::from_millis(10))
                    .expect("register");
            }

            let runner = tokio::spawn(coordinator.run(move |id| {
                if switched_for_cb.load(Ordering::SeqCst) == 1 {
                    *counts_for_cb
                        .lock()
                        .unwrap()
                        .entry(id.to_string())
                        .or_insert(0) += 1;
                }
            }));

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Narrow to a panel set, then restore everything.
            handle
                .set_poll_set(vec!["cpu".to_string()])
                .expect("narrow");
            tokio::time::sleep(Duration::from_millis(60)).await;

            handle
                .set_poll_set(COMPONENTS.iter().map(|s| s.to_string()).collect())
                .expect("restore");
            switched.store(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(150)).await;

            runner.abort();
        });

        for id in COMPONENTS {
            assert!(
                count(&counts, id) > 0,
                "{id} must resume polling when the full set is restored: {:?}",
                snapshot(&counts)
            );
        }
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
    /// While true the component is never handed to the refresh callback.
    ///
    /// Used by panel mode: a component whose elements are all disabled is not
    /// polled at all, rather than polled and thrown away. `last_refresh` is
    /// deliberately NOT reset while paused, so resuming does not fire a burst of
    /// immediate refreshes for every component at once.
    paused: bool,
}

impl ComponentRefresh {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_refresh: None, // None = never refreshed, needs immediate refresh
            paused: false,
        }
    }

    fn should_refresh(&self) -> bool {
        if self.paused {
            return false;
        }
        match self.last_refresh {
            None => true, // First refresh is immediate
            Some(last) => last.elapsed() >= self.interval,
        }
    }

    fn mark_refreshed(&mut self) {
        self.last_refresh = Some(Instant::now());
    }

    fn time_until_refresh(&self) -> Duration {
        // A paused component must not drag the coordinator's sleep to zero, or
        // the loop would spin on it forever. Report "nothing to do soon".
        if self.paused {
            return Duration::from_secs(3600);
        }
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
    /// Set the components that should be polled; all others are paused.
    ///
    /// Sending it twice with the same set is idempotent, and sending it with the
    /// full component list is how normal mode is restored.
    SetPaused(Vec<String>),
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
            let sleep_duration = self
                .components
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
                        CoordinatorCommand::SetPaused(poll_set) => {
                            // `poll_set` names the components that SHOULD be
                            // polled, so every component NOT in it is paused.
                            //
                            // One command carries the whole set, which keeps the
                            // coordinator stateless about modes and makes the
                            // command safe to send on every mode switch:
                            // switching panel -> normal is just "poll all of
                            // them", with no remembered previous set.
                            let mut newly_paused = Vec::new();
                            let mut resumed = Vec::new();
                            for (id, component) in self.components.iter_mut() {
                                let should_poll = poll_set.iter().any(|name| name == id);
                                if !should_poll && !component.paused {
                                    component.paused = true;
                                    newly_paused.push(id.clone());
                                } else if should_poll && component.paused {
                                    component.paused = false;
                                    resumed.push(id.clone());
                                }
                            }
                            log::info!(
                                "poll set updated: paused={:?} resumed={:?}",
                                newly_paused,
                                resumed
                            );
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

    /// Poll exactly `components`; pause every other registered component.
    ///
    /// Used by panel mode. Passing an empty set pauses everything, which is the
    /// correct behaviour for a panel whose elements are all disabled: nothing is
    /// being drawn, so nothing needs polling.
    pub fn set_poll_set(&self, components: Vec<String>) -> Result<()> {
        self.command_tx
            .send(CoordinatorCommand::SetPaused(components))
            .map_err(|e| anyhow::anyhow!("Failed to update poll set: {}", e))
    }
}
