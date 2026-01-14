use tray_icon::{
    TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};
use tuxedo_common::types::Profile;

pub struct SystemTray {
    _tray_icon: tray_icon::TrayIcon,
    menu_rx: std::sync::mpsc::Receiver<MenuEvent>,
    profile_items: Vec<MenuItem>,
    show_item: MenuItem,
    statistics_item: MenuItem,
    quit_item: MenuItem,
}

impl SystemTray {
    pub fn new(profiles: &[Profile], current_profile: &str) -> anyhow::Result<Self> {
        // Create menu
        let menu = Menu::new();

        // Add profile submenu
        let profiles_menu = Menu::new();
        let mut profile_items = Vec::new();

        for profile in profiles {
            let item = MenuItem::new(
                &profile.name,
                profile.name == current_profile,
                None
            );
            profiles_menu.append(&item)?;
            profile_items.push(item);
        }

        let profiles_submenu = Submenu::with_items(
            "Profiles",
            true,
            &profile_items,
        )?;
        menu.append(&profiles_submenu)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Quick actions
        let show_item = MenuItem::new("Show Window", true, None);
        menu.append(&show_item)?;

        let statistics_item = MenuItem::new("Statistics", true, None);
        menu.append(&statistics_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        let quit_item = MenuItem::new("Quit", true, None);
        menu.append(&quit_item)?;

        // Build tray icon
        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("TUXEDO Control Center")
            .with_icon(load_tray_icon())
            .build()?;

        let menu_rx = MenuEvent::receiver();

        Ok(Self {
            _tray_icon: tray_icon,
            menu_rx: menu_rx.clone(),
            profile_items,
            show_item,
            statistics_item,
            quit_item,
        })
    }

    pub fn handle_events(&mut self) -> Option<TrayEvent> {
        // Check for tray icon clicks
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click { button, button_state, .. } => {
                    if button == MouseButton::Left && button_state == MouseButtonState::Up {
                        return Some(TrayEvent::ShowWindow);
                    }
                }
                _ => {}
            }
        }

        // Check for menu events
        if let Ok(event) = self.menu_rx.try_recv() {
            let id = event.id();

            // Check profile items
            for (idx, item) in self.profile_items.iter().enumerate() {
                if item.id() == id {
                    return Some(TrayEvent::SwitchProfile(idx));
                }
            }

            if self.show_item.id() == id {
                return Some(TrayEvent::ShowWindow);
            }

            if self.statistics_item.id() == id {
                return Some(TrayEvent::ShowStatistics);
            }

            if self.quit_item.id() == id {
                return Some(TrayEvent::Quit);
            }
        }

        None
    }

    pub fn update_profiles(&mut self, profiles: &[Profile], current: &str) -> anyhow::Result<()> {
        // Rebuild menu with new profiles
        // This is a simplified version - full implementation would update existing menu
        Ok(())
    }
}

pub enum TrayEvent {
    ShowWindow,
    HideWindow,
    SwitchProfile(usize),
    ShowStatistics,
    Quit,
}

fn load_tray_icon() -> tray_icon::Icon {
    // Load from embedded bytes or file
    // For now, use a simple placeholder
    let rgba = vec![255u8; 32 * 32 * 4];  // White 32x32
    tray_icon::Icon::from_rgba(rgba, 32, 32).unwrap()
}
