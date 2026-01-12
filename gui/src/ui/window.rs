use gtk::prelude::*;
use gtk::{Application, Box, Orientation};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::Config;
use crate::dbus_client::DbusClient;
use super::{statistics_page, profiles_page, tuning_page, settings_page};

pub struct TuxedoWindow {
    window: adw::ApplicationWindow,
}

impl TuxedoWindow {
    pub fn new(
        app: &Application,
        config: Rc<RefCell<Config>>,
        dbus_client: Rc<RefCell<Option<DbusClient>>>,
    ) -> Self {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("TUXEDO Control Center")
            .default_width(1000)
            .default_height(700)
            .build();
        
        // Create header bar
        let header = adw::HeaderBar::new();
        
        // Create view stack and switcher
        let view_stack = adw::ViewStack::new();
        view_stack.set_vexpand(true);
        view_stack.set_hexpand(true);
        let view_switcher = adw::ViewSwitcher::builder()
            .stack(&view_stack)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .build();
        
        header.set_title_widget(Some(&view_switcher));
        
        // Create pages
        let statistics = statistics_page::create_page(config.clone(), dbus_client.clone());
        let profiles = profiles_page::create_page(config.clone(), dbus_client.clone());
        let tuning = tuning_page::create_page(config.clone(), dbus_client.clone());
        let settings = settings_page::create_page(config.clone());
        
        view_stack.add_titled(&statistics, Some("statistics"), "Statistics");
        view_stack.add_titled(&profiles, Some("profiles"), "Profiles");
        view_stack.add_titled(&tuning, Some("tuning"), "Tuning");
        view_stack.add_titled(&settings, Some("settings"), "Settings");
        
        // Create main layout
        let main_box = Box::new(Orientation::Vertical, 0);
        main_box.append(&header);
        main_box.append(&view_stack);
        
        window.set_content(Some(&main_box));
        
        // Handle close button
        window.connect_close_request(|window| {
            window.set_visible(false);
            gtk::glib::Propagation::Stop
        });
        
        Self { window }
    }
    
    pub fn present(&self) {
        self.window.present();
    }
}
