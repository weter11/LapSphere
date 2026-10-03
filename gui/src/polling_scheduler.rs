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
