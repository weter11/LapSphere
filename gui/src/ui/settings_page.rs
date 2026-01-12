use gtk::prelude::*;
use gtk::{Box, Orientation, ScrolledWindow};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::Config;

pub fn create_page(config: Rc<RefCell<Config>>) -> ScrolledWindow {
    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    
    let main_box = Box::new(Orientation::Vertical, 12);
    main_box.set_margin_top(24);
    main_box.set_margin_bottom(24);
    main_box.set_margin_start(24);
    main_box.set_margin_end(24);
    
    // Appearance Group
    let appearance_group = create_appearance_group(config.clone());
    main_box.append(&appearance_group);
    
    // Startup Group
    let startup_group = create_startup_group(config.clone());
    main_box.append(&startup_group);
    
    // Daemon Controls Group
    let daemon_group = create_daemon_group(config.clone());
    main_box.append(&daemon_group);
    
    // CPU Scheduler Group
    let scheduler_group = create_scheduler_group(config.clone());
    main_box.append(&scheduler_group);
    
    // Statistics Page Layout Group
    let stats_layout_group = create_stats_layout_group(config.clone());
    main_box.append(&stats_layout_group);
    
    scrolled.set_child(Some(&main_box));
    scrolled
}

fn create_appearance_group(config: Rc<RefCell<Config>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Appearance")
        .build();
    
    let theme_row = adw::ComboRow::builder()
        .title("Theme")
        .build();
    
    let theme_model = gtk::StringList::new(&["Auto", "Light", "Dark"]);
    theme_row.set_model(Some(&theme_model));
    
    let current_theme = match config.borrow().data.theme {
        tuxedo_common::types::Theme::Auto => 0,
        tuxedo_common::types::Theme::Light => 1,
        tuxedo_common::types::Theme::Dark => 2,
    };
    theme_row.set_selected(current_theme);
    
    let config_clone = config.clone();
    theme_row.connect_selected_notify(move |row| {
        let theme = match row.selected() {
            0 => tuxedo_common::types::Theme::Auto,
            1 => tuxedo_common::types::Theme::Light,
            2 => tuxedo_common::types::Theme::Dark,
            _ => tuxedo_common::types::Theme::Auto,
        };
        
        config_clone.borrow_mut().data.theme = theme.clone();
        let _ = config_clone.borrow().save();
        
        // Apply theme IMMEDIATELY
        let style_manager = adw::StyleManager::default();
        match theme {
            tuxedo_common::types::Theme::Auto => {
                style_manager.set_color_scheme(adw::ColorScheme::Default);
            },
            tuxedo_common::types::Theme::Light => {
                style_manager.set_color_scheme(adw::ColorScheme::ForceLight);
            },
            tuxedo_common::types::Theme::Dark => {
                style_manager.set_color_scheme(adw::ColorScheme::ForceDark);
            },
        }
    });
    
    group.add(&theme_row);
    group
}

fn create_startup_group(config: Rc<RefCell<Config>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Startup")
        .build();
    
    let minimized_row = adw::SwitchRow::builder()
        .title("Start minimized")
        .subtitle("Start in system tray")
        .build();
    minimized_row.set_active(config.borrow().data.start_minimized);
    
    let config_clone = config.clone();
    minimized_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.start_minimized = row.is_active();
        let _ = config_clone.borrow().save();
        // Applied immediately - no restart needed
    });
    
    group.add(&minimized_row);
    
    let autostart_row = adw::SwitchRow::builder()
        .title("Enable autostart")
        .subtitle("Launch on system boot")
        .build();
    autostart_row.set_active(config.borrow().data.autostart);
    
    let config_clone = config.clone();
    autostart_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.autostart = row.is_active();
        let _ = config_clone.borrow().save();
        
        // Applied IMMEDIATELY
        if row.is_active() {
            let _ = create_autostart_file();
        } else {
            let _ = remove_autostart_file();
        }
    });
    
    group.add(&autostart_row);
    group
}

fn create_daemon_group(config: Rc<RefCell<Config>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Daemon Controls")
        .build();
    
    let fan_daemon_row = adw::SwitchRow::builder()
        .title("Fan daemon")
        .subtitle("Monitor temperatures and apply fan curves")
        .build();
    fan_daemon_row.set_active(config.borrow().data.fan_daemon_enabled);
    
    let config_clone = config.clone();
    fan_daemon_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.fan_daemon_enabled = row.is_active();
        let _ = config_clone.borrow().save();
        // Applied immediately
    });
    
    group.add(&fan_daemon_row);
    
    let app_monitoring_row = adw::SwitchRow::builder()
        .title("App monitoring")
        .subtitle("Enable automatic profile switching")
        .build();
    app_monitoring_row.set_active(config.borrow().data.app_monitoring_enabled);
    
    let config_clone = config.clone();
    app_monitoring_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.app_monitoring_enabled = row.is_active();
        let _ = config_clone.borrow().save();
        // Applied immediately
    });
    
    group.add(&app_monitoring_row);
    group
}

fn create_scheduler_group(config: Rc<RefCell<Config>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("CPU Scheduler")
        .description("Global scheduler setting (not per-profile)")
        .build();
    
    let scheduler_row = adw::ComboRow::builder()
        .title("Scheduler")
        .build();
    
    let scheduler_model = gtk::StringList::new(&["CFS", "EEVDF"]);
    scheduler_row.set_model(Some(&scheduler_model));
    scheduler_row.set_selected(if config.borrow().data.cpu_scheduler == "CFS" { 0 } else { 1 });
    
    let config_clone = config.clone();
    scheduler_row.connect_selected_notify(move |row| {
        let scheduler = if row.selected() == 0 { "CFS" } else { "EEVDF" };
        config_clone.borrow_mut().data.cpu_scheduler = scheduler.to_string();
        let _ = config_clone.borrow().save();
        // Applied immediately
    });
    
    group.add(&scheduler_row);
    group
}

fn create_stats_layout_group(config: Rc<RefCell<Config>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Statistics Page Layout")
        .description("Changes apply immediately")
        .build();
    
    // System Info
    let show_system_info_row = adw::SwitchRow::builder()
        .title("Show system info")
        .build();
    show_system_info_row.set_active(config.borrow().data.statistics_sections.show_system_info);
    
    let config_clone = config.clone();
    show_system_info_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.statistics_sections.show_system_info = row.is_active();
        let _ = config_clone.borrow().save();
        // Applied immediately - page will update on next view
    });
    
    group.add(&show_system_info_row);
    
    // CPU
    let show_cpu_row = adw::SwitchRow::builder()
        .title("Show CPU")
        .build();
    show_cpu_row.set_active(config.borrow().data.statistics_sections.show_cpu);
    
    let config_clone = config.clone();
    show_cpu_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.statistics_sections.show_cpu = row.is_active();
        let _ = config_clone.borrow().save();
        // Applied immediately
    });
    
    group.add(&show_cpu_row);
    
    // GPU
    let show_gpu_row = adw::SwitchRow::builder()
        .title("Show GPU")
        .build();
    show_gpu_row.set_active(config.borrow().data.statistics_sections.show_gpu);
    
    let config_clone = config.clone();
    show_gpu_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.statistics_sections.show_gpu = row.is_active();
        let _ = config_clone.borrow().save();
    });
    
    group.add(&show_gpu_row);
    
    // Battery
    let show_battery_row = adw::SwitchRow::builder()
        .title("Show battery")
        .build();
    show_battery_row.set_active(config.borrow().data.statistics_sections.show_battery);
    
    let config_clone = config.clone();
    show_battery_row.connect_active_notify(move |row| {
        config_clone.borrow_mut().data.statistics_sections.show_battery = row.is_active();
        let _ = config_clone.borrow().save();
    });
    
    group.add(&show_battery_row);
    
    group
}

fn create_autostart_file() -> std::io::Result<()> {
    use std::fs;
    use std::path::PathBuf;
    
    let home = std::env::var("HOME").unwrap_or_default();
    let autostart_dir = PathBuf::from(&home).join(".config/autostart");
    fs::create_dir_all(&autostart_dir)?;
    
    let desktop_content = r#"[Desktop Entry]
Type=Application
Name=TUXEDO Control Center
Exec=tuxedo-control-center --minimized
Icon=preferences-system
Comment=Hardware control for TUXEDO/Clevo laptops
X-GNOME-Autostart-enabled=true
"#;
    
    let autostart_file = autostart_dir.join("tuxedo-control-center.desktop");
    fs::write(autostart_file, desktop_content)?;
    
    Ok(())
}

fn remove_autostart_file() -> std::io::Result<()> {
    use std::fs;
    use std::path::PathBuf;
    
    let home = std::env::var("HOME").unwrap_or_default();
    let autostart_file = PathBuf::from(&home)
        .join(".config/autostart/tuxedo-control-center.desktop");
    
    if autostart_file.exists() {
        fs::remove_file(autostart_file)?;
    }
    
    Ok(())
}
