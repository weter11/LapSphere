use anyhow::Result;
use ksni::{menu, Tray, TrayService};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tuxedo_common::types::Profile;

// Communication from UI to tray
pub enum TrayCommand {
    UpdateProfiles(Vec<Profile>, String),
}

// Communication from tray to UI
#[derive(Debug, PartialEq, Eq)]
pub enum TrayEvent {
    ShowWindow,
    Quit,
    SwitchProfile(String),
}

#[derive(Debug)]
struct TuxedoTray {
    event_tx: mpsc::UnboundedSender<TrayEvent>,
    profiles: Arc<Mutex<Vec<Profile>>>,
    current_profile: Arc<Mutex<String>>,
}

impl Tray for TuxedoTray {
    fn icon_name(&self) -> String {
        "tuxedo-control-center".into()
    }

    fn title(&self) -> String {
        "TUXEDO Control Center".into()
    }

    fn menu(&self) -> Vec<menu::MenuItem> {
        let profiles = self.profiles.blocking_lock();
        let current_profile = self.current_profile.blocking_lock();

        let mut profile_items: Vec<menu::MenuItem> = profiles
            .iter()
            .map(|p| {
                let name = p.name.clone();
                let current = current_profile.clone();
                let tx = self.event_tx.clone();

                menu::MenuItem::radio(
                    &p.name,
                    p.name == *current,
                    Box::new(move |_| {
                        let _ = tx.send(TrayEvent::SwitchProfile(name.clone()));
                    }),
                )
            })
            .collect();

        if !profile_items.is_empty() {
            profile_items.push(menu::MenuItem::separator());
        }

        let tx = self.event_tx.clone();
        profile_items.extend(vec![
            menu::MenuItem::new(
                "Show",
                Box::new(move |_| {
                    let _ = tx.send(TrayEvent::ShowWindow);
                }),
            ),
            menu::MenuItem::separator(),
            menu::MenuItem::new(
                "Quit",
                Box::new({
                    let tx = self.event_tx.clone();
                    move |_| {
                        let _ = tx.send(TrayEvent::Quit);
                    }
                }),
            ),
        ]);

        profile_items
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.event_tx.send(TrayEvent::ShowWindow);
    }
}

pub async fn run_tray_service(
    mut command_rx: mpsc::UnboundedReceiver<TrayCommand>,
    event_tx: mpsc::UnboundedSender<TrayEvent>,
    initial_profiles: Vec<Profile>,
    initial_profile_name: String,
) -> Result<()> {
    let profiles = Arc::new(Mutex::new(initial_profiles));
    let current_profile = Arc::new(Mutex::new(initial_profile_name));

    let tray = TuxedoTray {
        event_tx: event_tx.clone(),
        profiles: profiles.clone(),
        current_profile: current_profile.clone(),
    };

    let service = TrayService::new(tray);
    let handle = service.handle();
    service.spawn();

    while let Some(command) = command_rx.recv().await {
        match command {
            TrayCommand::UpdateProfiles(new_profiles, new_current) => {
                *profiles.lock().await = new_profiles;
                *current_profile.lock().await = new_current;
                handle.update_menu();
            }
        }
    }

    Ok(())
}