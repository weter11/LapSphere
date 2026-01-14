use crate::app::HardwareUpdate;
use crate::dbus_client::DbusClient;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

type PollFn = Pin<Box<dyn Future<Output = ()> + Send>>;

pub enum SchedulerCommand {
    UpdateRates(HashMap<String, u64>),
}

pub struct PollJob {
    name: String,
    interval: Duration,
    next_run: Instant,
    poll_fn: Box<dyn Fn(DbusClient, mpsc::UnboundedSender<HardwareUpdate>) -> PollFn + Send>,
}

pub struct Scheduler {
    jobs: Vec<PollJob>,
    client: DbusClient,
    tx: mpsc::UnboundedSender<HardwareUpdate>,
    command_rx: mpsc::UnboundedReceiver<SchedulerCommand>,
}

impl Scheduler {
    pub fn new(
        client: DbusClient,
        tx: mpsc::UnboundedSender<HardwareUpdate>,
        command_rx: mpsc::UnboundedReceiver<SchedulerCommand>,
        config: &tuxedo_common::types::AppConfig,
    ) -> Self {
        let jobs = vec![
            PollJob {
                name: "cpu".to_string(),
                interval: Duration::from_millis(config.statistics_sections.cpu_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_cpu_info().await {
                            let _ = tx.send(HardwareUpdate::CpuInfo(info));
                        }
                    })
                }),
            },
            PollJob {
                name: "gpu".to_string(),
                interval: Duration::from_millis(config.statistics_sections.gpu_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_gpu_info().await {
                            let _ = tx.send(HardwareUpdate::GpuInfo(info));
                        }
                    })
                }),
            },
            PollJob {
                name: "memory".to_string(),
                interval: Duration::from_millis(config.statistics_sections.cpu_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_memory_info().await {
                            let _ = tx.send(HardwareUpdate::MemoryInfo(info));
                        }
                    })
                }),
            },
            PollJob {
                name: "fans".to_string(),
                interval: Duration::from_millis(config.statistics_sections.fans_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_fan_info().await {
                            let _ = tx.send(HardwareUpdate::FanInfo(info));
                        }
                    })
                }),
            },
            PollJob {
                name: "battery".to_string(),
                interval: Duration::from_millis(config.statistics_sections.battery_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_battery_info().await {
                            let _ = tx.send(HardwareUpdate::BatteryInfo(info));
                        }
                    })
                }),
            },
            PollJob {
                name: "wifi".to_string(),
                interval: Duration::from_millis(config.statistics_sections.wifi_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_wifi_info().await {
                            let _ = tx.send(HardwareUpdate::WifiInfo(info));
                        }
                    })
                }),
            },
            PollJob {
                name: "storage".to_string(),
                interval: Duration::from_millis(config.statistics_sections.storage_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_storage_device_info().await {
                            let _ = tx.send(HardwareUpdate::StorageDeviceInfo(info));
                        }
                    })
                }),
            },
            PollJob {
                name: "mounts".to_string(),
                interval: Duration::from_millis(config.statistics_sections.storage_poll_rate),
                next_run: Instant::now(),
                poll_fn: Box::new(|client, tx| {
                    Box::pin(async move {
                        if let Ok(Ok(info)) = client.get_mount_info().await {
                            let _ = tx.send(HardwareUpdate::MountInfo(info));
                        }
                    })
                }),
            },
        ];

        Self { jobs, client, tx, command_rx }
    }

    pub async fn run(mut self) {
        loop {
            // Check for commands without blocking
            if let Ok(command) = self.command_rx.try_recv() {
                match command {
                    SchedulerCommand::UpdateRates(rates) => self.update_rates(&rates),
                }
            }

            // Find the job with the nearest run time
            let nearest_job_time = self.jobs.iter().map(|j| j.next_run).min();

            if let Some(next_run) = nearest_job_time {
                let now = Instant::now();
                if next_run > now {
                    // Sleep until the next job is ready, but also check for commands periodically
                    let sleep_duration = (next_run - now).min(Duration::from_millis(500));
                    tokio::time::sleep(sleep_duration).await;
                }

                // Check again for any job that's ready to run
                let now = Instant::now();
                for job in self.jobs.iter_mut() {
                    if job.next_run <= now {
                        let future = (job.poll_fn)(self.client.clone(), self.tx.clone());
                        tokio::spawn(future); // Run in a separate task to avoid blocking scheduler
                        job.next_run = now + job.interval;
                    }
                }
            } else {
                // No jobs, sleep for a bit
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    fn update_rates(&mut self, rates: &HashMap<String, u64>) {
        for (name, millis) in rates {
            if let Some(job) = self.jobs.iter_mut().find(|j| j.name == *name) {
                job.interval = Duration::from_millis(*millis);
            }
        }
    }
}
