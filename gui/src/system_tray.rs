use ksni::{Tray, TrayMethods, menu::{StandardItem}};
use std::sync::mpsc;
use tuxedo_common::types::Profile;

pub enum TrayEvent {
    ShowWindow,
    Quit,
    SwitchProfile(String),
}

pub struct SystemTray {
    tx: mpsc::Sender<TrayEvent>,
    profiles: Vec<Profile>,
    current_profile: String,
}

impl Tray for SystemTray {
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayEvent::ShowWindow);
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn id(&self) -> String {
        "tuxedo-control-center".to_string()
    }

    fn title(&self) -> String {
        "TUXEDO Control Center".to_string()
    }

    fn icon_name(&self) -> String {
        "tuxedo-control-center".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut menu = vec![
            StandardItem {
                label: "Show".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayEvent::ShowWindow);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
        ];

        for profile in &self.profiles {
            let profile_name = profile.name.clone();
            menu.push(
                StandardItem {
                    label: profile.name.clone(),
                    activate: Box::new(move |this: &mut Self| {
                        let _ = this.tx.send(TrayEvent::SwitchProfile(profile_name.clone()));
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        menu
    }
}

pub async fn create_tray_service(profiles: &[Profile], current_profile: &str, tx: mpsc::Sender<TrayEvent>) {
    let tray = SystemTray {
        tx,
        profiles: profiles.to_vec(),
        current_profile: current_profile.to_string(),
    };
    if let Err(e) = tray.spawn().await {
        log::error!("Failed to spawn tray service: {}", e);
    }
}
