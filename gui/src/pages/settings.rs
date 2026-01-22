use egui::{Ui, ScrollArea, RichText, Slider, ComboBox, Context, Grid};
use crate::app::{AppState, SettingsTab};
use crate::dbus_client::DbusClient;
use crate::theme::TuxedoTheme;
use crate::pages::statistics::{normalize_section_order, STATISTICS_SECTIONS};

const STORAGE_POLL_MIN_SECONDS: f32 = 0.5;
const STORAGE_POLL_MAX_SECONDS: f32 = 10.0;

pub fn draw(ui: &mut Ui, state: &mut AppState, theme: &mut TuxedoTheme, ctx: &Context, dbus_client: Option<&DbusClient>) {
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.settings_tab, SettingsTab::Main, "Main");
        ui.selectable_value(&mut state.settings_tab, SettingsTab::StatsConfiguration, "Stats configuration");
        ui.selectable_value(&mut state.settings_tab, SettingsTab::Hardware, "Hardware info");
    });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            match state.settings_tab {
                SettingsTab::Main => draw_main_settings(ui, state, theme, ctx, dbus_client),
                SettingsTab::StatsConfiguration => draw_stats_configuration(ui, state),
                SettingsTab::Hardware => draw_hardware_info(ui, state),
            }
        });
}

fn draw_main_settings(ui: &mut Ui, state: &mut AppState, theme: &mut TuxedoTheme, ctx: &Context, dbus_client: Option<&DbusClient>) {
    // Appearance
    ui.label(RichText::new("Appearance").strong().heading());
    ui.add_space(8.0);
    
    ui.horizontal(|ui| {
        ui.label("Theme:");
        
        use tuxedo_common::types::Theme;
        let mut theme_changed = false;
        let mut new_theme = state.config.theme.clone();
        
        if ui.selectable_value(&mut new_theme, Theme::Auto, "Auto").clicked() {
            theme_changed = true;
        }
        if ui.selectable_value(&mut new_theme, Theme::Light, "Light").clicked() {
            theme_changed = true;
        }
        if ui.selectable_value(&mut new_theme, Theme::Dark, "Dark").clicked() {
            theme_changed = true;
        }
        
        if theme_changed {
            state.config.theme = new_theme.clone();
            let _ = state.save_settings();
            
            // Apply theme immediately
            *theme = TuxedoTheme::new(&new_theme);
            theme.apply(ctx);
        }
    });
    
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);
    
    // Font Size
    ui.label(RichText::new("Font Size").strong().heading());
    ui.add_space(8.0);
    
    ui.horizontal(|ui| {
        ui.label("UI Font Size:");
        
        use tuxedo_common::types::FontSize;
        let mut font_changed = false;
        let mut new_font = state.config.font_size.clone();
        
        if ui.selectable_value(&mut new_font, FontSize::Small, "Small").clicked() {
            font_changed = true;
        }
        if ui.selectable_value(&mut new_font, FontSize::Medium, "Medium").clicked() {
            font_changed = true;
        }
        if ui.selectable_value(&mut new_font, FontSize::Large, "Large").clicked() {
            font_changed = true;
        }
        
        if font_changed {
            state.config.font_size = new_font.clone();
            let _ = state.save_settings();
            
            // Apply font size immediately
            apply_font_size(ctx, &new_font);
        }
    });
    
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);
    
    // Startup
    ui.label(RichText::new("Startup").strong().heading());
    ui.add_space(8.0);
    
    if ui.checkbox(&mut state.config.start_minimized, "Start minimized").changed() {
        let _ = state.save_settings();
    }

    if ui.checkbox(&mut state.config.tray_enabled, "Tray (minimize on close)").changed() {
        let _ = state.save_settings();
    }
    
    if ui.checkbox(&mut state.config.autostart, "Enable autostart").changed() {
        let _ = state.save_settings();
        // TODO: Create/remove autostart file
    }
    
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);
    
    // Daemon Controls
    ui.label(RichText::new("Daemon Controls").strong().heading());
    ui.add_space(8.0);
    
    if ui.checkbox(&mut state.config.fan_daemon_enabled, "Fan daemon").changed() {
        let _ = state.save_settings();
    }
    ui.label(RichText::new("Monitor temperatures and apply fan curves").small().italics());
    ui.add_space(6.0);
    
    if ui.checkbox(&mut state.config.app_monitoring_enabled, "App monitoring").changed() {
        let _ = state.save_settings();
    }
    ui.label(RichText::new("Monitor running applications for automatic profile switching").small().italics());
    
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);
    
    // Battery Charge Control
    draw_battery_settings(ui, state);
    
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);
    
    // NVIDIA Prime Profile
    draw_prime_profile_settings(ui, state, dbus_client);
}

fn draw_stats_configuration(ui: &mut Ui, state: &mut AppState) {
    // Statistics Page Layout
    ui.label(RichText::new("Statistics Page Layout").strong().heading());
    ui.add_space(8.0);
    
    if ui.checkbox(&mut state.config.statistics_sections.show_system_info, "Show system info").changed() {
        let _ = state.save_settings();
    }
    if ui.checkbox(&mut state.config.statistics_sections.show_cpu, "Show CPU").changed() {
        let _ = state.save_settings();
    }
    if ui.checkbox(&mut state.config.statistics_sections.show_memory, "Show memory").changed() {
        let _ = state.save_settings();
    }
    if ui.checkbox(&mut state.config.statistics_sections.show_gpu, "Show GPU").changed() {
        let _ = state.save_settings();
    }
    if ui.checkbox(&mut state.config.statistics_sections.show_battery, "Show battery").changed() {
        let _ = state.save_settings();
    }
    if ui.checkbox(&mut state.config.statistics_sections.show_wifi, "Show WiFi").changed() {
        let _ = state.save_settings();
    }
    if ui.checkbox(&mut state.config.statistics_sections.show_storage, "Show storage").changed() {
        let _ = state.save_settings();
    }
    if ui.checkbox(&mut state.config.statistics_sections.show_fans, "Show fans").changed() {
        let _ = state.save_settings();
    }
    
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("Section Order").strong().heading());
    ui.add_space(8.0);

    let normalized = normalize_section_order(&state.config.statistics_sections.section_order);
    if normalized != state.config.statistics_sections.section_order {
        state.config.statistics_sections.section_order = normalized;
    }

    let total_sections = state.config.statistics_sections.section_order.len();
    let mut move_request = None;
    for (index, section) in state.config.statistics_sections.section_order.iter().enumerate() {
        let label = STATISTICS_SECTIONS
            .iter()
            .find(|(key, _)| key == section)
            .map(|(_, label)| *label)
            .unwrap_or(section.as_str());
        ui.horizontal(|ui| {
            ui.label(label);
            if ui.add_enabled(index > 0, egui::Button::new("⬆")).clicked() {
                move_request = Some((index, index - 1));
            }
            if ui.add_enabled(index + 1 < total_sections, egui::Button::new("⬇")).clicked() {
                move_request = Some((index, index + 1));
            }
        });
    }

    if let Some((from, to)) = move_request {
        state.config.statistics_sections.section_order.swap(from, to);
        let _ = state.save_settings();
    }
    
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);
    
    // Polling Rates
    ui.label(RichText::new("Polling Rates").strong().heading());
    ui.add_space(8.0);
    ui.label(RichText::new("How often to update each section (in seconds)").small().italics());
    ui.add_space(6.0);
    
    let mut cpu_poll = (state.config.statistics_sections.cpu_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("CPU:");
        if ui.add(Slider::new(&mut cpu_poll, 0.5..=10.0).step_by(0.5).suffix(" s")).changed() {
            let new_rate = (cpu_poll * 1000.0) as u64;
            state.config.statistics_sections.cpu_poll_rate = new_rate;
            let _ = state.save_settings();
            // Update coordinator interval
            if let Some(ref handle) = state.coordinator_handle {
                let _ = handle.update_interval("cpu".to_string(), std::time::Duration::from_millis(new_rate));
            }
        }
    });

    let mut memory_poll = (state.config.statistics_sections.memory_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("Memory:");
        if ui.add(Slider::new(&mut memory_poll, 0.5..=10.0).step_by(0.5).suffix(" s")).changed() {
            let new_rate = (memory_poll * 1000.0) as u64;
            state.config.statistics_sections.memory_poll_rate = new_rate;
            let _ = state.save_settings();
            // Update coordinator interval
            if let Some(ref handle) = state.coordinator_handle {
                let _ = handle.update_interval("memory".to_string(), std::time::Duration::from_millis(new_rate));
            }
        }
    });
    
    let mut gpu_poll = (state.config.statistics_sections.gpu_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("GPU:");
        if ui.add(Slider::new(&mut gpu_poll, 0.5..=10.0).step_by(0.5).suffix(" s")).changed() {
            let new_rate = (gpu_poll * 1000.0) as u64;
            state.config.statistics_sections.gpu_poll_rate = new_rate;
            let _ = state.save_settings();
            // Update coordinator interval
            if let Some(ref handle) = state.coordinator_handle {
                let _ = handle.update_interval("gpu".to_string(), std::time::Duration::from_millis(new_rate));
            }
        }
    });
    
    let mut battery_poll = (state.config.statistics_sections.battery_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("Battery:");
        if ui.add(Slider::new(&mut battery_poll, 0.5..=30.0).step_by(0.5).suffix(" s")).changed() {
            let new_rate = (battery_poll * 1000.0) as u64;
            state.config.statistics_sections.battery_poll_rate = new_rate;
            let _ = state.save_settings();
            // Update coordinator interval
            if let Some(ref handle) = state.coordinator_handle {
                let _ = handle.update_interval("battery".to_string(), std::time::Duration::from_millis(new_rate));
            }
        }
    });
    
    let mut wifi_poll = (state.config.statistics_sections.wifi_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("WiFi:");
        if ui.add(Slider::new(&mut wifi_poll, 0.5..=30.0).step_by(0.5).suffix(" s")).changed() {
            let new_rate = (wifi_poll * 1000.0) as u64;
            state.config.statistics_sections.wifi_poll_rate = new_rate;
            let _ = state.save_settings();
            // Update coordinator interval
            if let Some(ref handle) = state.coordinator_handle {
                let _ = handle.update_interval("wifi".to_string(), std::time::Duration::from_millis(new_rate));
            }
        }
    });
    
    let mut storage_poll = (state.config.statistics_sections.storage_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("Storage:");
        if ui.add(Slider::new(&mut storage_poll, STORAGE_POLL_MIN_SECONDS..=STORAGE_POLL_MAX_SECONDS)
            .step_by(0.5)
            .suffix(" s"))
            .changed()
        {
            let new_rate = (storage_poll * 1000.0) as u64;
            state.config.statistics_sections.storage_poll_rate = new_rate;
            let _ = state.save_settings();
            // Update coordinator interval
            if let Some(ref handle) = state.coordinator_handle {
                let _ = handle.update_interval("storage".to_string(), std::time::Duration::from_millis(new_rate));
                let _ = handle.update_interval("mount".to_string(), std::time::Duration::from_millis(new_rate));
            }
        }
    });
    
    let mut fans_poll = (state.config.statistics_sections.fans_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("Fans:");
        if ui.add(Slider::new(&mut fans_poll, 0.5..=10.0).step_by(0.5).suffix(" s")).changed() {
            let new_rate = (fans_poll * 1000.0) as u64;
            state.config.statistics_sections.fans_poll_rate = new_rate;
            let _ = state.save_settings();
            // Update coordinator interval
            if let Some(ref handle) = state.coordinator_handle {
                let _ = handle.update_interval("fans".to_string(), std::time::Duration::from_millis(new_rate));
            }
        }
    });
}

fn draw_hardware_info(ui: &mut Ui, state: &AppState) {
    let interface_label = hardware_interface_label(state.hardware_interface.as_deref());

    ui.label(RichText::new("System").strong().heading());
    ui.add_space(8.0);
    if let Some(info) = &state.system_info {
        Grid::new("hardware_system_grid")
            .num_columns(2)
            .spacing([40.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Model:");
                ui.label(&info.product_name);
                ui.end_row();

                ui.label("SKU:");
                ui.label(&info.product_sku);
                ui.end_row();

                ui.label("Board:");
                ui.label(&info.board_name);
                ui.end_row();

                ui.label("Manufacturer:");
                ui.label(&info.manufacturer);
                ui.end_row();

                ui.label("BIOS Version:");
                ui.label(&info.bios_version);
                ui.end_row();

                ui.label("Laptop Type:");
                ui.label(interface_label);
                ui.end_row();

                ui.label("TUXEDO Kernel Modules:");
                ui.label(&info.tuxedo_kernel_modules);
                ui.end_row();
            });
    } else {
        ui.label("System information unavailable.");
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("Keyboard").strong().heading());
    ui.add_space(8.0);
    if let Some(caps) = &state.keyboard_capabilities {
        Grid::new("hardware_keyboard_grid")
            .num_columns(2)
            .spacing([40.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Detected Type:");
                ui.label(format!("{:?}", caps.keyboard_type));
                ui.end_row();

                ui.label("Zones:");
                ui.label(caps.num_zones.to_string());
                ui.end_row();

                ui.label("Supports Brightness:");
                ui.label(if caps.supports_brightness { "Yes" } else { "No" });
                ui.end_row();

                ui.label("Supports Color:");
                ui.label(if caps.supports_color { "Yes" } else { "No" });
                ui.end_row();

                ui.label("Supports Effects:");
                ui.label(if caps.supports_effects { "Yes" } else { "No" });
                ui.end_row();
            });
    } else {
        ui.label("Keyboard capabilities information unavailable.");
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("CPU").strong().heading());
    ui.add_space(8.0);
    if let Some(cpu) = &state.cpu_info {
        Grid::new("hardware_cpu_grid")
            .num_columns(2)
            .spacing([40.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Model:");
                ui.label(&cpu.name);
                ui.end_row();

                ui.label("Cores:");
                ui.label(cpu.cores.len().to_string());
                ui.end_row();

                if cpu.capabilities.has_scaling_driver {
                    ui.label("Scaling Driver:");
                    ui.label(&cpu.scaling_driver);
                    ui.end_row();
                }

                if cpu.capabilities.has_scaling_governor {
                    ui.label("Governor:");
                    ui.label(&cpu.governor);
                    ui.end_row();
                }
            });
    } else {
        ui.label("CPU information unavailable.");
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("Memory").strong().heading());
    ui.add_space(8.0);
    if let Some(memory) = &state.memory_info {
        Grid::new("hardware_memory_grid")
            .num_columns(2)
            .spacing([40.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Total:");
                ui.label(format!("{:.2} GiB", memory.total_gib));
                ui.end_row();
            });
    } else {
        ui.label("Memory information unavailable.");
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("GPU").strong().heading());
    ui.add_space(8.0);
    if state.gpu_info.is_empty() {
        ui.label("GPU information unavailable.");
    } else {
        for (idx, gpu) in state.gpu_info.iter().enumerate() {
            ui.label(RichText::new(&gpu.name).strong());
            Grid::new(format!("hardware_gpu_grid_{}", idx))
                .num_columns(2)
                .spacing([40.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Type:");
                    ui.label(format!("{:?}", gpu.gpu_type));
                    ui.end_row();
                });

            if idx + 1 < state.gpu_info.len() {
                ui.add_space(8.0);
            }
        }
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("Storage").strong().heading());
    ui.add_space(8.0);
    if state.storage_device_info.is_empty() {
        ui.label("Storage information unavailable.");
    } else {
        for (idx, device) in state.storage_device_info.iter().enumerate() {
            ui.label(RichText::new(&device.model).strong());
            Grid::new(format!("hardware_storage_grid_{}", idx))
                .num_columns(2)
                .spacing([40.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Size:");
                    ui.label(format!("{} GB", device.size_gb));
                    ui.end_row();
                });

            if idx + 1 < state.storage_device_info.len() {
                ui.add_space(8.0);
            }
        }
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("WiFi").strong().heading());
    ui.add_space(8.0);
    if state.wifi_info.is_empty() {
        ui.label("WiFi information unavailable.");
    } else {
        for (idx, wifi) in state.wifi_info.iter().enumerate() {
            ui.label(RichText::new(&wifi.interface).strong());
            Grid::new(format!("hardware_wifi_grid_{}", idx))
                .num_columns(2)
                .spacing([40.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Network controller:");
                    if let Some(ctrl) = wifi.network_controller.as_deref() {
                        ui.label(ctrl);
                    } else {
                        ui.label(RichText::new("—").weak());
                    }
                    ui.end_row();

                    ui.label("Subsystem:");
                    if let Some(sub) = wifi.subsystem.as_deref() {
                        ui.label(sub);
                    } else {
                        ui.label(RichText::new("—").weak());
                    }
                    ui.end_row();

                    ui.label("Driver:");
                    ui.label(&wifi.driver);
                    ui.end_row();

                    ui.label("Driver Version:");
                    if let Some(version) = wifi.driver_version.as_deref() {
                        ui.label(version);
                    } else {
                        ui.label(RichText::new("—").weak());
                    }
                    ui.end_row();

                    ui.label("Firmware:");
                    if let Some(version) = wifi.firmware_version.as_deref() {
                        ui.label(version);
                    } else {
                        ui.label(RichText::new("—").weak());
                    }
                    ui.end_row();

                });

            if idx + 1 < state.wifi_info.len() {
                ui.add_space(8.0);
            }
        }
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("Battery").strong().heading());
    ui.add_space(8.0);
    if let Some(battery) = &state.battery_info {
        Grid::new("hardware_battery_grid")
            .num_columns(2)
            .spacing([40.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Capacity:");
                ui.label(format!("{} mAh", battery.capacity_mah));
                ui.end_row();
            });
    } else {
        ui.label("Battery information unavailable.");
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(16.0);

    ui.label(RichText::new("Fans").strong().heading());
    ui.add_space(8.0);
    let fan_mode = state
        .current_profile()
        .map(|profile| if profile.fan_settings.control_enabled { "Manual" } else { "Auto" })
        .unwrap_or("Auto");
    Grid::new("hardware_fans_grid")
        .num_columns(2)
        .spacing([40.0, 8.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label("Mode:");
            ui.label(fan_mode);
            ui.end_row();

            ui.label("Fan Count:");
            ui.label(state.fan_info.len().to_string());
            ui.end_row();
        });
}

fn hardware_interface_label(interface: Option<&str>) -> &'static str {
    match interface {
        Some(info) if info.contains("Clevo") => "Clevo",
        Some(info) if info.contains("Uniwill") => "Uniwill",
        _ => "Unknown",
    }
}

fn draw_battery_settings(ui: &mut Ui, state: &mut AppState) {
    ui.heading("🔋 Battery Charge Control");
    ui.add_space(8.0);

    if ui.checkbox(&mut state.config.battery_settings.control_enabled, "Enable charge thresholds").changed() {
        let _ = state.save_settings();
    }
    ui.add_space(6.0);

    if state.config.battery_settings.control_enabled {
        // Start Threshold
        ui.horizontal(|ui| {
            ui.label("Start Threshold:");
            if ComboBox::from_id_source("start_threshold_combo")
                .selected_text(format!("{}%", state.config.battery_settings.charge_start_threshold))
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    for &threshold in &state.available_start_thresholds {
                        if ui.selectable_value(
                            &mut state.config.battery_settings.charge_start_threshold,
                            threshold,
                            format!("{}%", threshold),
                        ).clicked() {
                            changed = true;
                        }
                    }
                    changed
                }).inner.unwrap_or(false) 
            {
                let _ = state.save_settings();
            }
        });

        // End Threshold
        ui.horizontal(|ui| {
            ui.label("End Threshold:");
            if ComboBox::from_id_source("end_threshold_combo")
                .selected_text(format!("{}%", state.config.battery_settings.charge_end_threshold))
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    for &threshold in &state.available_end_thresholds {
                        if ui.selectable_value(
                            &mut state.config.battery_settings.charge_end_threshold,
                            threshold,
                            format!("{}%", threshold),
                        ).clicked() {
                            changed = true;
                        }
                    }
                    changed
                }).inner.unwrap_or(false)
            {
                let _ = state.save_settings();
            }
        });

        // Validate thresholds
        if state.config.battery_settings.charge_start_threshold >= state.config.battery_settings.charge_end_threshold {
            if let Some(valid_start) = state.available_start_thresholds.iter()
                .filter(|&&t| t < state.config.battery_settings.charge_end_threshold)
                .last()
            {
                state.config.battery_settings.charge_start_threshold = *valid_start;
            }
        }

        // Apply button
        ui.add_space(6.0);
        if ui.button("💾 Apply Battery Settings").clicked() {
            // Create DBus client and apply settings
            if let Ok(client) = crate::dbus_client::DbusClient::new() {
                let settings = state.config.battery_settings.clone();
                tokio::spawn(async move {
                    let rx = client.set_battery_settings(settings);
                    let _ = rx.await;
                });
                state.show_message("Battery settings applied", false);
            }
        }
    }
}

fn apply_font_size(ctx: &Context, font_size: &tuxedo_common::types::FontSize) {
    use egui::{FontId, FontFamily, TextStyle};
    use tuxedo_common::types::FontSize;
    
    let mut style = (*ctx.style()).clone();
    
    let (heading, body, button, small, mono) = match font_size {
        FontSize::Small => (18.0, 12.0, 12.0, 9.0, 11.0),
        FontSize::Medium => (22.0, 14.0, 14.0, 11.0, 13.0),
        FontSize::Large => (26.0, 16.0, 16.0, 13.0, 15.0),
    };
    
    style.text_styles = [
        (TextStyle::Heading, FontId::new(heading, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(body, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(mono, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(button, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(small, FontFamily::Proportional)),
    ].into();
    
    ctx.set_style(style);
}

fn draw_prime_profile_settings(ui: &mut Ui, state: &mut AppState, dbus_client: Option<&DbusClient>) {
    ui.heading("🎮 NVIDIA Prime Profile");
    ui.add_space(8.0);
    
    // Check if NVIDIA GPU is present
    let has_nvidia = state.gpu_info.iter().any(|g| g.name.contains("NVIDIA"));
    
    if !has_nvidia {
        ui.label("NVIDIA GPU not detected. Prime profile switching is only available for NVIDIA GPUs.");
        return;
    }
    
    ui.label(RichText::new("⚠️ Changing the Prime profile requires a system restart to take effect.")
        .small()
        .italics());
    ui.add_space(6.0);
    
    // Get current saved profile from config
    let current_profile = state.current_profile()
        .and_then(|p| p.gpu_settings.prime_profile.clone())
        .unwrap_or_else(|| "on-demand".to_string());
    
    // Initialize pending selection if not set
    if state.pending_prime_profile.is_none() {
        state.pending_prime_profile = Some(current_profile.clone());
    }
    
    // Use the pending selection for the combo box
    let mut selected_profile = state.pending_prime_profile.clone().unwrap_or(current_profile.clone());
    
    ui.horizontal(|ui| {
        ui.label("Current Profile:");
        ComboBox::from_id_source("settings_prime_profile_combo")
            .selected_text(&selected_profile)
            .show_ui(ui, |ui| {
                if ui.selectable_value(&mut selected_profile, "on-demand".to_string(), "On-Demand").clicked() {
                    state.pending_prime_profile = Some("on-demand".to_string());
                }
                if ui.selectable_value(&mut selected_profile, "nvidia".to_string(), "NVIDIA").clicked() {
                    state.pending_prime_profile = Some("nvidia".to_string());
                }
                if ui.selectable_value(&mut selected_profile, "intel".to_string(), "Intel").clicked() {
                    state.pending_prime_profile = Some("intel".to_string());
                }
            });
    });
    
    // Update pending selection if changed
    if state.pending_prime_profile.as_ref() != Some(&selected_profile) {
        state.pending_prime_profile = Some(selected_profile.clone());
    }
    
    ui.add_space(8.0);
    
    // Description of each mode
    ui.vertical(|ui| {
        match selected_profile.as_str() {
            "on-demand" => {
                ui.label(RichText::new("On-Demand: The discrete GPU is used for demanding applications while the integrated GPU handles normal tasks. Best balance of performance and power efficiency.").small());
            }
            "nvidia" => {
                ui.label(RichText::new("NVIDIA: Always use the discrete NVIDIA GPU. Maximum performance but higher power consumption.").small());
            }
            "intel" => {
                ui.label(RichText::new("Intel: Always use the integrated Intel GPU. Disables the NVIDIA GPU for maximum battery life.").small());
            }
            _ => {}
        }
    });
    
    ui.add_space(12.0);
    
    // Show Apply button if selection differs from saved config
    if selected_profile != current_profile {
        ui.horizontal(|ui| {
            if ui.button("💾 Apply Prime Profile").clicked() {
                // Update the GPU settings in all profiles
                for profile in &mut state.config.profiles {
                    profile.gpu_settings.prime_profile = Some(selected_profile.clone());
                }
                let _ = state.save_profiles();
                
                // Apply via dbus
                if let Some(client) = dbus_client {
                    let _ = client.set_prime_profile(&selected_profile);
                }
                
                // Reset pending to new value
                state.pending_prime_profile = Some(selected_profile.clone());
                
                state.show_message("Prime profile applied. Please restart your laptop for changes to take effect.", false);
            }
        });
        
        ui.add_space(8.0);
        
        // Two-step restart confirmation
        if !state.restart_confirmation_pending {
            if ui.button("🔄 Restart Laptop...").clicked() {
                state.restart_confirmation_pending = true;
            }
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Are you sure you want to restart now?").strong());
            });
            ui.horizontal(|ui| {
                if ui.button("✅ Yes, Restart Now").clicked() {
                    state.restart_confirmation_pending = false;
                    // Trigger system restart via systemctl (requires polkit authorization)
                    let _ = std::process::Command::new("systemctl")
                        .args(["reboot"])
                        .spawn();
                }
                if ui.button("❌ Cancel").clicked() {
                    state.restart_confirmation_pending = false;
                }
            });
            
            ui.label(RichText::new("⚠️ This will immediately reboot your laptop! Save all work first.")
                .small()
                .color(egui::Color32::from_rgb(255, 100, 100)));
        }
    }
}
