use tray_item::{TrayItem, IconSource};
use tuxedo_common::types::Profile;
use std::sync::mpsc;

pub struct SystemTray {
    _tray: TrayItem,
    pub rx: mpsc::Receiver<TrayEvent>,
}

#[derive(Debug)]
pub enum TrayEvent {
    ShowWindow,
    Quit,
    SwitchProfile(String),
}

impl SystemTray {
    pub fn new(profiles: &[Profile], current_profile: &str) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let mut tray = TrayItem::new("TUXEDO Control Center", IconSource::Resource("tray-icon"))?;

        let show_tx = tx.clone();
        tray.add_menu_item("Show", move || {
            let _ = show_tx.send(TrayEvent::ShowWindow);
        })?;

        let quit_tx = tx.clone();
        tray.add_menu_item("Quit", move || {
            let _ = quit_tx.send(TrayEvent::Quit);
        })?;

        for profile in profiles {
            let profile_name = profile.name.clone();
            let profile_tx = tx.clone();
            tray.add_menu_item(&profile.name, move || {
                let _ = profile_tx.send(TrayEvent::SwitchProfile(profile_name.clone()));
            })?;
        }

        Ok(Self {
            _tray: tray,
            rx,
        })
    }
}
