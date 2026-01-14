use egui::{Ui, ScrollArea, RichText, Slider, ComboBox, Context};
use crate::app::AppState;
use crate::dbus_client::DbusClient;
use crate::theme::TuxedoTheme;

pub fn draw(ui: &mut Ui, state: &mut AppState, theme: &mut TuxedoTheme, ctx: &Context, dbus_client: Option<&DbusClient>) {
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            ui.heading("⚙️ Settings");
            ui.add_space(16.0);
            
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
                    let _ = state.save_config();
                    
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
                    let _ = state.save_config();
                    
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
                let _ = state.save_config();
            }
            
            if ui.checkbox(&mut state.config.autostart, "Enable autostart").changed() {
                let _ = state.save_config();
                // TODO: Create/remove autostart file
            }
            
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(16.0);
            
            // Daemon Controls
            ui.label(RichText::new("Daemon Controls").strong().heading());
            ui.add_space(8.0);
            
            if ui.checkbox(&mut state.config.fan_daemon_enabled, "Fan daemon").changed() {
                let _ = state.save_config();
            }
            ui.label(RichText::new("Monitor temperatures and apply fan curves").small().italics());
            ui.add_space(6.0);
            
            if ui.checkbox(&mut state.config.app_monitoring_enabled, "App monitoring").changed() {
                let _ = state.save_config();
            }
            ui.label(RichText::new("Monitor running applications for automatic profile switching").small().italics());
            
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(16.0);
            
            // Statistics Page Layout
            ui.label(RichText::new("Statistics Page Layout").strong().heading());
            ui.add_space(8.0);
            
            if ui.checkbox(&mut state.config.statistics_sections.show_system_info, "Show system info").changed() {
                let _ = state.save_config();
            }
            if ui.checkbox(&mut state.config.statistics_sections.show_cpu, "Show CPU").changed() {
                let _ = state.save_config();
            }
            if ui.checkbox(&mut state.config.statistics_sections.show_gpu, "Show GPU").changed() {
                let _ = state.save_config();
            }
            if ui.checkbox(&mut state.config.statistics_sections.show_battery, "Show battery").changed() {
                let _ = state.save_config();
            }
            if ui.checkbox(&mut state.config.statistics_sections.show_wifi, "Show WiFi").changed() {
                let _ = state.save_config();
            }
            if ui.checkbox(&mut state.config.statistics_sections.show_storage, "Show storage").changed() {
                let _ = state.save_config();
            }
            if ui.checkbox(&mut state.config.statistics_sections.show_fans, "Show fans").changed() {
                let _ = state.save_config();
            }
            
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
                    let _ = state.save_config();
                    // Update coordinator interval
                    if let Some(ref handle) = state.coordinator_handle {
                        let _ = handle.update_interval("cpu".to_string(), std::time::Duration::from_millis(new_rate));
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
                    let _ = state.save_config();
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
                    let _ = state.save_config();
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
                    let _ = state.save_config();
                    // Update coordinator interval
                    if let Some(ref handle) = state.coordinator_handle {
                        let _ = handle.update_interval("wifi".to_string(), std::time::Duration::from_millis(new_rate));
                    }
                }
            });
            
            let mut storage_poll = (state.config.statistics_sections.storage_poll_rate as f32) / 1000.0;
            ui.horizontal(|ui| {
                ui.label("Storage:");
                if ui.add(Slider::new(&mut storage_poll, 5.0..=60.0).step_by(0.5).suffix(" s")).changed() {
                    let new_rate = (storage_poll * 1000.0) as u64;
                    state.config.statistics_sections.storage_poll_rate = new_rate;
                    let _ = state.save_config();
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
                    let _ = state.save_config();
                    // Update coordinator interval
                    if let Some(ref handle) = state.coordinator_handle {
                        let _ = handle.update_interval("fans".to_string(), std::time::Duration::from_millis(new_rate));
                    }
                }
            });
        });
}

fn draw_battery_settings(ui: &mut Ui, state: &mut AppState) {
    ui.heading("🔋 Battery Charge Control");
    ui.add_space(8.0);

    if ui.checkbox(&mut state.config.battery_settings.control_enabled, "Enable charge thresholds").changed() {
        let _ = state.save_config();
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
                let _ = state.save_config();
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
                let _ = state.save_config();
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
                let _ = state.save_config();
                
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
