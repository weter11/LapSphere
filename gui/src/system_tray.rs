use ksni::{
    menu::{MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu},
    ToolTip,
};
use ksni::blocking::TrayMethods;
use std::sync::mpsc;
use tuxedo_common::types::Profile;

const TRAY_ICON_SIZE: i32 = 32; // ksni::Icon uses i32 width/height (DBus int32).
const TRAY_ICON_BYTES_PER_PIXEL: usize = 4;
const TRAY_ICON_BYTE_LEN: usize =
    (TRAY_ICON_SIZE as usize) * (TRAY_ICON_SIZE as usize) * TRAY_ICON_BYTES_PER_PIXEL;

struct TrayState {
    profiles: Vec<String>,
    current_profile: usize,
    tx: mpsc::Sender<TrayEvent>,
}

impl TrayState {
    fn send_event(&self, event: TrayEvent) {
        if self.tx.send(event).is_err() {
            log::warn!("System tray event channel closed.");
        }
    }
}

impl ksni::Tray for TrayState {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        "TUXEDO Control Center".into()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "TUXEDO Control Center".into(),
            ..Default::default()
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        load_tray_icon()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send_event(TrayEvent::ShowWindow);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut items = Vec::new();

        if !self.profiles.is_empty() {
            // Clamp the selected index in case profiles changed since tray init.
            let selected = self.current_profile.min(self.profiles.len() - 1);
            let options = self
                .profiles
                .iter()
                .map(|name| RadioItem {
                    label: name.clone(),
                    ..Default::default()
                })
                .collect();

            items.push(
                SubMenu {
                    label: "Profiles".into(),
                    submenu: vec![RadioGroup {
                        selected,
                        select: Box::new(|tray: &mut Self, index| {
                            tray.current_profile = index;
                            tray.send_event(TrayEvent::SwitchProfile(index));
                        }),
                        options,
                    }
                    .into()],
                    ..Default::default()
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }

        items.push(
            StandardItem {
                label: "Show Window".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.send_event(TrayEvent::ShowWindow);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            StandardItem {
                label: "Statistics".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.send_event(TrayEvent::ShowStatistics);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.send_event(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

pub struct SystemTray {
    _tray_handle: ksni::blocking::Handle<TrayState>,
    menu_rx: mpsc::Receiver<TrayEvent>,
}

impl SystemTray {
    pub fn new(profiles: &[Profile], current_profile: &str) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let profile_names: Vec<String> = profiles.iter().map(|profile| profile.name.clone()).collect();
        let current_index = profiles
            .iter()
            .position(|profile| profile.name == current_profile)
            .unwrap_or(0);

        let tray = TrayState {
            profiles: profile_names,
            current_profile: current_index,
            tx,
        };

        let tray_handle = tray.spawn()?;

        Ok(Self {
            _tray_handle: tray_handle,
            menu_rx: rx,
        })
    }

    pub fn handle_events(&mut self) -> Option<TrayEvent> {
        if let Ok(event) = self.menu_rx.try_recv() {
            return Some(event);
        }

        None
    }

    pub fn update_profiles(&mut self, profiles: &[Profile], current: &str) -> anyhow::Result<()> {
        let profile_names: Vec<String> = profiles.iter().map(|profile| profile.name.clone()).collect();
        let current_index = profiles
            .iter()
            .position(|profile| profile.name == current)
            .unwrap_or(0);

        self._tray_handle.update(|tray| {
            tray.profiles = profile_names;
            tray.current_profile = current_index;
        });

        Ok(())
    }
}

pub enum TrayEvent {
    ShowWindow,
    SwitchProfile(usize),
    ShowStatistics,
    Quit,
}

fn load_tray_icon() -> Vec<ksni::Icon> {
    // Placeholder icon until an embedded resource is wired in (ARGB, all channels 255).
    let argb = vec![255u8; TRAY_ICON_BYTE_LEN];

    vec![ksni::Icon {
        width: TRAY_ICON_SIZE,
        height: TRAY_ICON_SIZE,
        data: argb,
    }]
}
