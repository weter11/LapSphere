use gtk::prelude::*;
use gtk::{Box, Orientation, ProgressBar, ScrolledWindow};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use gtk::glib;

use crate::config::Config;
use crate::dbus_client::DbusClient;

pub fn create_page(
    config: Rc<RefCell<Config>>,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) -> ScrolledWindow {
    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    
    let main_box = Box::new(Orientation::Vertical, 12);
    main_box.set_margin_top(24);
    main_box.set_margin_bottom(24);
    main_box.set_margin_start(24);
    main_box.set_margin_end(24);
    
    let sections = &config.borrow().data.statistics_sections;
    
    if sections.show_system_info {
        let system_info = create_system_info_section(dbus_client.clone());
        main_box.append(&system_info);
    }
    
    if sections.show_cpu {
        let cpu_section = create_cpu_section(dbus_client.clone());
        main_box.append(&cpu_section);
        setup_cpu_realtime_updates(cpu_section.clone(), dbus_client.clone());
    }
    
    if sections.show_gpu {
        let gpu_section = create_gpu_section(dbus_client.clone());
        main_box.append(&gpu_section);
        setup_gpu_realtime_updates(gpu_section.clone(), dbus_client.clone());
    }
    
    if sections.show_battery {
        let battery_section = create_battery_section(dbus_client.clone());
        main_box.append(&battery_section);
        setup_battery_realtime_updates(battery_section.clone(), dbus_client.clone());
    }
    
    if sections.show_wifi {
        let wifi_section = create_wifi_section(dbus_client.clone());
        main_box.append(&wifi_section);
        setup_wifi_realtime_updates(wifi_section.clone(), dbus_client.clone());
    }
    
    if sections.show_fans {
        let fans_section = create_fans_section(dbus_client.clone());
        main_box.append(&fans_section);
        setup_fans_realtime_updates(fans_section.clone(), dbus_client.clone());
    }
    
    scrolled.set_child(Some(&main_box));
    scrolled
}

fn create_system_info_section(dbus_client: Rc<RefCell<Option<DbusClient>>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("System Information")
        .build();
    
    if let Some(client) = dbus_client.borrow().as_ref() {
        match client.get_system_info() {
            Ok(info) => {
                let product_row = adw::ActionRow::builder()
                    .title("Product")
                    .subtitle(&info.product_name)
                    .build();
                group.add(&product_row);
                
                let manufacturer_row = adw::ActionRow::builder()
                    .title("Manufacturer")
                    .subtitle(&info.manufacturer)
                    .build();
                group.add(&manufacturer_row);
                
                let bios_row = adw::ActionRow::builder()
                    .title("BIOS Version")
                    .subtitle(&info.bios_version)
                    .build();
                group.add(&bios_row);
            }
            Err(e) => {
                let error_row = adw::ActionRow::builder()
                    .title("Error loading system info")
                    .subtitle(&e.to_string())
                    .build();
                group.add(&error_row);
            }
        }
    }
    
    group
}

fn create_cpu_section(dbus_client: Rc<RefCell<Option<DbusClient>>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("CPU")
        .build();
    
    let name_row = adw::ActionRow::builder()
        .title("Processor")
        .subtitle("Loading...")
        .build();
    group.add(&name_row);
    
    let freq_row = adw::ActionRow::builder()
        .title("Frequency")
        .subtitle("Loading...")
        .build();
    group.add(&freq_row);
    
    let temp_row = adw::ActionRow::builder()
        .title("Temperature")
        .subtitle("Loading...")
        .build();
    group.add(&temp_row);
    
    let governor_row = adw::ActionRow::builder()
        .title("Governor")
        .subtitle("Loading...")
        .build();
    group.add(&governor_row);
    
    update_cpu_info(&name_row, &freq_row, &temp_row, &governor_row, dbus_client);
    
    group
}

fn update_cpu_info(
    name_row: &adw::ActionRow,
    freq_row: &adw::ActionRow,
    temp_row: &adw::ActionRow,
    governor_row: &adw::ActionRow,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) {
    if let Some(client) = dbus_client.borrow().as_ref() {
        match client.get_cpu_info() {
            Ok(info) => {
                name_row.set_subtitle(&info.name);
                freq_row.set_subtitle(&format!("{} MHz", info.median_frequency / 1000));
                temp_row.set_subtitle(&format!("{:.1}°C", info.package_temp));
                governor_row.set_subtitle(&info.governor);
            }
            Err(e) => {
                name_row.set_subtitle(&format!("Error: {}", e));
            }
        }
    }
}

fn create_gpu_section(dbus_client: Rc<RefCell<Option<DbusClient>>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("GPU Information")
        .build();
    
    if let Some(client) = dbus_client.borrow().as_ref() {
        match client.get_gpu_info() {
            Ok(gpus) => {
                for gpu in gpus {
                    let status_text = match gpu.status {
                        tuxedo_common::types::GpuStatus::Active => "🟢 Active",
                        tuxedo_common::types::GpuStatus::Suspended => "🔴 Suspended",
                        tuxedo_common::types::GpuStatus::Unknown => "⚪ Unknown",
                    };
                    
                    let gpu_row = adw::ActionRow::builder()
                        .title(&gpu.name)
                        .subtitle(&format!("Status: {} | Driver: {}", 
                            status_text,
                            gpu.driver.as_ref().unwrap_or(&"Unknown".to_string())))
                        .build();
                    
                    if let Some(ref pci_id) = gpu.pci_id {
                        let detail = gtk::Label::new(Some(&format!("PCI: {}", pci_id)));
                        detail.set_css_classes(&["dim-label", "caption"]);
                        gpu_row.add_suffix(&detail);
                    }
                    
                    group.add(&gpu_row);
                }
            }
            Err(e) => {
                let error_row = adw::ActionRow::builder()
                    .title("Error loading GPU info")
                    .subtitle(&e.to_string())
                    .build();
                group.add(&error_row);
            }
        }
    }
    
    group
}

fn create_battery_section(dbus_client: Rc<RefCell<Option<DbusClient>>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Battery")
        .build();
    
    let charge_row = adw::ActionRow::builder()
        .title("Charge Level")
        .subtitle("Loading...")
        .build();
    group.add(&charge_row);
    
    let status_row = adw::ActionRow::builder()
        .title("Status")
        .subtitle("Loading...")
        .build();
    group.add(&status_row);
    
    let power_row = adw::ActionRow::builder()
        .title("Power Draw")
        .subtitle("Loading...")
        .build();
    group.add(&power_row);
    
    let health_row = adw::ActionRow::builder()
        .title("Health Estimate")
        .subtitle("Calculating...")
        .build();
    group.add(&health_row);
    
    update_battery_info(&charge_row, &status_row, &power_row, &health_row, dbus_client);
    
    group
}

fn update_battery_info(
    charge_row: &adw::ActionRow,
    status_row: &adw::ActionRow,
    power_row: &adw::ActionRow,
    health_row: &adw::ActionRow,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) {
    if let Some(client) = dbus_client.borrow().as_ref() {
        match client.get_battery_info() {
            Ok(info) => {
                // Charge with visual bar
                charge_row.set_subtitle(&format!("{}%", info.charge_percent));
                let charge_bar = ProgressBar::new();
                charge_bar.set_fraction(info.charge_percent as f64 / 100.0);
                if info.charge_percent >= 80 {
                    charge_bar.add_css_class("success");
                } else if info.charge_percent >= 20 {
                    charge_bar.remove_css_class("error");
                } else {
                    charge_bar.add_css_class("error");
                }
                charge_row.add_suffix(&charge_bar);
                
                // Status with time estimate
                let status_text = match info.status {
                    tuxedo_common::types::BatteryStatus::Charging => "⚡ Charging".to_string(),
                    tuxedo_common::types::BatteryStatus::Discharging => {
                        if let Some(draw) = info.power_draw_w {
                            if draw > 0.0 {
                                let remaining_wh = info.capacity_mah as f32 * info.voltage_mv as f32 / 1_000_000.0;
                                let hours = remaining_wh / draw * (info.charge_percent as f32 / 100.0);
                                format!("🔋 Discharging (~{:.1}h)", hours)
                            } else {
                                "🔋 Discharging".to_string()
                            }
                        } else {
                            "🔋 Discharging".to_string()
                        }
                    }
                    tuxedo_common::types::BatteryStatus::Full => "✓ Full".to_string(),
                    tuxedo_common::types::BatteryStatus::NotCharging => "⏸ Not Charging".to_string(),
                    tuxedo_common::types::BatteryStatus::Unknown => "❓ Unknown".to_string(),
                };
                status_row.set_subtitle(&status_text);
                
                // Power draw
                if info.on_ac_power {
                    power_row.set_subtitle("N/A (on AC power)");
                } else if let Some(draw) = info.power_draw_w {
                    power_row.set_subtitle(&format!("{:.1} W", draw));
                } else {
                    power_row.set_subtitle("Unknown");
                }
                
                // Health indicator (simplified - would need design capacity)
                let health_percent = 95; // Placeholder
                let health_icon = if health_percent >= 90 {
                    "🟢"
                } else if health_percent >= 80 {
                    "🟡"
                } else {
                    "🔴"
                };
                health_row.set_subtitle(&format!("{} {}% capacity", health_icon, health_percent));
            }
            Err(e) => {
                charge_row.set_subtitle(&format!("Error: {}", e));
            }
        }
    }
}

fn create_wifi_section(dbus_client: Rc<RefCell<Option<DbusClient>>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("WiFi")
        .build();
    
    if let Some(client) = dbus_client.borrow().as_ref() {
        match client.get_wifi_info() {
            Ok(interfaces) => {
                if interfaces.is_empty() {
                    let no_wifi = adw::ActionRow::builder()
                        .title("No WiFi interfaces found")
                        .build();
                    group.add(&no_wifi);
                } else {
                    for iface in interfaces {
                        let expander = adw::ExpanderRow::builder()
                            .title(&iface.interface)
                            .subtitle(&format!("Driver: {}", iface.driver))
                            .build();
                        
                        // SSID
                        if let Some(ref ssid) = iface.ssid {
                            let ssid_row = adw::ActionRow::builder()
                                .title("Connected to")
                                .subtitle(ssid)
                                .build();
                            expander.add_row(&ssid_row);
                        }
                        
                        // Signal quality with visual indicator
                        if let Some(signal) = iface.signal_strength {
                            let quality_row = create_wifi_quality_row(signal);
                            expander.add_row(&quality_row);
                        }
                        
                        // Link speed
                        if let Some(speed) = iface.link_speed_mbps {
                            let speed_row = adw::ActionRow::builder()
                                .title("Link Speed")
                                .subtitle(&format!("{} Mbps", speed))
                                .build();
                            expander.add_row(&speed_row);
                        }
                        
                        // Temperature
                        if let Some(temp) = iface.temperature {
                            let temp_row = adw::ActionRow::builder()
                                .title("Temperature")
                                .subtitle(&format!("{:.1}°C", temp))
                                .build();
                            expander.add_row(&temp_row);
                        }
                        
                        group.add(&expander);
                    }
                }
            }
            Err(e) => {
                let error_row = adw::ActionRow::builder()
                    .title("Error loading WiFi info")
                    .subtitle(&e.to_string())
                    .build();
                group.add(&error_row);
            }
        }
    }
    
    group
}

fn create_wifi_quality_row(signal_dbm: i32) -> adw::ActionRow {
    // Convert dBm to quality
    let quality_percent = if signal_dbm >= -50 {
        100
    } else if signal_dbm >= -67 {
        (2 * (signal_dbm + 100)) as u32
    } else if signal_dbm >= -80 {
        ((signal_dbm + 100) - 10) as u32
    } else {
        0
    }.min(100);
    
    let (quality_text, quality_icon) = match quality_percent {
        90..=100 => ("Excellent", "🟢"),
        70..=89 => ("Very Good", "🟢"),
        50..=69 => ("Good", "🟡"),
        30..=49 => ("Fair", "🟠"),
        _ => ("Poor", "🔴"),
    };
    
    let row = adw::ActionRow::builder()
        .title("Signal Quality")
        .subtitle(&format!("{} {} ({} dBm, {}%)", quality_icon, quality_text, signal_dbm, quality_percent))
        .build();
    
    // Visual quality bar
    let quality_bar = ProgressBar::new();
    quality_bar.set_fraction(quality_percent as f64 / 100.0);
    quality_bar.set_show_text(false);
    quality_bar.set_hexpand(false);
    quality_bar.set_size_request(100, -1);
    
    if quality_percent >= 70 {
        quality_bar.add_css_class("success");
    } else if quality_percent >= 50 {
        quality_bar.add_css_class("warning");
    } else {
        quality_bar.add_css_class("error");
    }
    
    row.add_suffix(&quality_bar);
    row
}

fn create_fans_section(dbus_client: Rc<RefCell<Option<DbusClient>>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Fans")
        .build();
    
    if let Some(client) = dbus_client.borrow().as_ref() {
        match client.get_fan_info() {
            Ok(fans) => {
                if fans.is_empty() {
                    let no_fans = adw::ActionRow::builder()
                        .title("No fan information available")
                        .subtitle("Fan control may not be supported on this hardware")
                        .build();
                    group.add(&no_fans);
                } else {
                    for fan in fans {
                        let fan_row = adw::ActionRow::builder()
                            .title(&fan.name)
                            .subtitle(&format!(
                                "Speed: {}% | RPM: {}",
                                fan.duty_percent,
                                fan.rpm.map(|r| r.to_string()).unwrap_or_else(|| "N/A".to_string())
                            ))
                            .build();
                        
                        if let Some(temp) = fan.temperature {
                            let temp_label = gtk::Label::new(Some(&format!("{:.1}°C", temp)));
                            temp_label.set_css_classes(&["dim-label"]);
                            fan_row.add_suffix(&temp_label);
                        }
                        
                        group.add(&fan_row);
                    }
                }
            }
            Err(e) => {
                let error_row = adw::ActionRow::builder()
                    .title("Error loading fan info")
                    .subtitle(&e.to_string())
                    .build();
                group.add(&error_row);
            }
        }
    }
    
    group
}

// Real-time update functions

fn setup_cpu_realtime_updates(
    group: adw::PreferencesGroup,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) {
    let name_row = find_row_by_title(&group, "Processor");
    let freq_row = find_row_by_title(&group, "Frequency");
    let temp_row = find_row_by_title(&group, "Temperature");
    let gov_row = find_row_by_title(&group, "Governor");
    
    if let (Some(nr), Some(fr), Some(tr), Some(gr)) = (name_row, freq_row, temp_row, gov_row) {
        glib::timeout_add_seconds_local(2, move || {
            update_cpu_info(&nr, &fr, &tr, &gr, dbus_client.clone());
            glib::ControlFlow::Continue
        });
    }
}

fn setup_gpu_realtime_updates(
    _group: adw::PreferencesGroup,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) {
    glib::timeout_add_seconds_local(3, move || {
        if let Some(client) = dbus_client.borrow().as_ref() {
            let _ = client.get_gpu_info();
        }
        glib::ControlFlow::Continue
    });
}

fn setup_battery_realtime_updates(
    group: adw::PreferencesGroup,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) {
    let charge_row = find_row_by_title(&group, "Charge Level");
    let status_row = find_row_by_title(&group, "Status");
    let power_row = find_row_by_title(&group, "Power Draw");
    let health_row = find_row_by_title(&group, "Health Estimate");
    
    if let (Some(cr), Some(sr), Some(pr), Some(hr)) = (charge_row, status_row, power_row, health_row) {
        glib::timeout_add_seconds_local(2, move || {
            update_battery_info(&cr, &sr, &pr, &hr, dbus_client.clone());
            glib::ControlFlow::Continue
        });
    }
}

fn setup_wifi_realtime_updates(
    _group: adw::PreferencesGroup,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) {
    glib::timeout_add_seconds_local(5, move || {
        if let Some(client) = dbus_client.borrow().as_ref() {
            let _ = client.get_wifi_info();
        }
        glib::ControlFlow::Continue
    });
}

fn setup_fans_realtime_updates(
    _group: adw::PreferencesGroup,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) {
    glib::timeout_add_seconds_local(2, move || {
        if let Some(client) = dbus_client.borrow().as_ref() {
            let _ = client.get_fan_info();
        }
        glib::ControlFlow::Continue
    });
}

fn find_row_by_title(group: &adw::PreferencesGroup, title: &str) -> Option<adw::ActionRow> {
    let mut child = group.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if let Ok(row) = widget.downcast::<adw::ActionRow>() {
            if row.title() == title {
                return Some(row);
            }
        } 
        child = next;
        }
    None
}
