use anyhow::Result;
use ksni::{
    menu::{RadioGroup, RadioItem, StandardItem},
    Tray, TrayMethods,
};
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
pub struct TuxedoTray {
    event_tx: mpsc::UnboundedSender<TrayEvent>,
    profiles: Arc<Mutex<Vec<Profile>>>,
    current_profile: Arc<Mutex<String>>,
}

impl Tray for TuxedoTray {
    fn id(&self) -> String {
        "tuxedo-control-center".into()
    }

    fn icon_name(&self) -> String {
        "tuxedo-control-center".into()
    }

    fn title(&self) -> String {
        "TUXEDO Control Center".into()
    }

    fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
        let profiles = self.profiles.blocking_lock();
        let current_profile = self.current_profile.blocking_lock();
        let current_profile_index = profiles
            .iter()
            .position(|p| p.name == *current_profile)
            .unwrap_or(0);

        let radio_group = RadioGroup {
            selected: current_profile_index,
            options: profiles
                .iter()
                .map(|p| RadioItem {
                    label: p.name.clone(),
                    ..Default::default()
                })
                .collect(),
            select: Box::new({
                let event_tx = self.event_tx.clone();
                let profiles = profiles.clone();
                move |_, selected_index| {
                    if let Some(profile) = profiles.get(selected_index) {
                        let _ = event_tx.send(TrayEvent::SwitchProfile(profile.name.clone()));
                    }
                }
            }),
            ..Default::default()
        };

        let mut menu = vec![ksni::menu::MenuItem::SubMenu(ksni::menu::SubMenu {
            label: "Profiles".to_string(),
            submenu: vec![radio_group.into()],
            ..Default::default()
        })];

        menu.extend(vec![
            ksni::menu::MenuItem::Separator,
            ksni::menu::MenuItem::Standard(StandardItem {
                label: "Show".into(),
                activate: Box::new({
                    let event_tx = self.event_tx.clone();
                    move |_| {
                        let _ = event_tx.send(TrayEvent::ShowWindow);
                    }
                }),
                ..Default::default()
            }),
            ksni::menu::MenuItem::Separator,
            ksni::menu::MenuItem::Standard(StandardItem {
                label: "Quit".into(),
                activate: Box::new({
                    let event_tx = self.event_tx.clone();
                    move |_| {
                        let _ = event_tx.send(TrayEvent::Quit);
                    }
                }),
                ..Default::default()
            }),
        ]);

        menu
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
        event_tx,
        profiles: profiles.clone(),
        current_profile: current_profile.clone(),
    };

    let handle = tray.spawn().await?;

    while let Some(command) = command_rx.recv().await {
        match command {
            TrayCommand::UpdateProfiles(new_profiles, new_current) => {
                *profiles.lock().await = new_profiles;
                *current_profile.lock().await = new_current;
                let _ = handle.update(|_tray: &mut TuxedoTray| {}).await;
            }
        }
    }

    Ok(())
}
