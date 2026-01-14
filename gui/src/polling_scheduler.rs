use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use anyhow::Result;

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
    last_refresh: Instant,
}

impl ComponentRefresh {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_refresh: Instant::now(),
        }
    }

    fn should_refresh(&self) -> bool {
        self.last_refresh.elapsed() >= self.interval
    }

    fn mark_refreshed(&mut self) {
        self.last_refresh = Instant::now();
    }

    fn time_until_refresh(&self) -> Duration {
        let elapsed = self.last_refresh.elapsed();
        if elapsed >= self.interval {
            Duration::from_millis(0)
        } else {
            self.interval - elapsed
        }
    }
}

/// Commands for the coordinator
pub enum CoordinatorCommand {
    /// Register a component with its refresh interval
    Register(String, Duration),
    /// Update component refresh interval
    UpdateInterval(String, Duration),
    /// Unregister a component
    Unregister(String),
    /// Shutdown coordinator
    Shutdown,
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
                        CoordinatorCommand::Unregister(id) => {
                            log::debug!("Unregistering component: {}", id);
                            self.components.remove(&id);
                        }
                        CoordinatorCommand::Shutdown => {
                            log::info!("Shutting down UI refresh coordinator");
                            break;
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

    /// Unregister a component
    pub fn unregister(&self, id: String) -> Result<()> {
        self.command_tx
            .send(CoordinatorCommand::Unregister(id))
            .map_err(|e| anyhow::anyhow!("Failed to unregister: {}", e))
    }

    /// Shutdown the coordinator
    pub fn shutdown(&self) -> Result<()> {
        self.command_tx
            .send(CoordinatorCommand::Shutdown)
            .map_err(|e| anyhow::anyhow!("Failed to shutdown: {}", e))
    }
}
