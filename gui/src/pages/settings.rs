use egui::{Ui, ScrollArea, RichText, Slider, ComboBox, Context};
use crate::app::AppState;
use crate::theme::TuxedoTheme;

pub fn draw(ui: &mut Ui, state: &mut AppState, theme: &mut TuxedoTheme, ctx: &Context) {
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
            
            // Polling Rates
            ui.label(RichText::new("Polling Rates").strong().heading());
            ui.add_space(8.0);
            ui.label(RichText::new("How often to update each section (in seconds)").small().italics());
            ui.add_space(6.0);
            
            let mut cpu_poll = (state.config.statistics_sections.cpu_poll_rate as f32) / 1000.0;
            ui.horizontal(|ui| {
                ui.label("CPU:");
                if ui.add(Slider::new(&mut cpu_poll, 0.5..=10.0).step_by(0.5).suffix(" s")).changed() {
                    state.config.statistics_sections.cpu_poll_rate = (cpu_poll * 1000.0) as u64;
                    let _ = state.save_config();
                }
            });
            
            let mut gpu_poll = (state.config.statistics_sections.gpu_poll_rate as f32) / 1000.0;
            ui.horizontal(|ui| {
                ui.label("GPU:");
                if ui.add(Slider::new(&mut gpu_poll, 0.5..=10.0).step_by(0.5).suffix(" s")).changed() {
                    state.config.statistics_sections.gpu_poll_rate = (gpu_poll * 1000.0) as u64;
                    let _ = state.save_config();
                }
            });
            
            let mut battery_poll = (state.config.statistics_sections.battery_poll_rate as f32) / 1000.0;
            ui.horizontal(|ui| {
                ui.label("Battery:");
                if ui.add(Slider::new(&mut battery_poll, 0.5..=30.0).step_by(0.5).suffix(" s")).changed() {
                    state.config.statistics_sections.battery_poll_rate = (battery_poll * 1000.0) as u64;
                    let _ = state.save_config();
                }
            });
            
            let mut wifi_poll = (state.config.statistics_sections.wifi_poll_rate as f32) / 1000.0;
            ui.horizontal(|ui| {
                ui.label("WiFi:");
                if ui.add(Slider::new(&mut wifi_poll, 0.5..=30.0).step_by(0.5).suffix(" s")).changed() {
                    state.config.statistics_sections.wifi_poll_rate = (wifi_poll * 1000.0) as u64;
                    let _ = state.save_config();
                }
            });
            
            let mut storage_poll = (state.config.statistics_sections.storage_poll_rate as f32) / 1000.0;
            ui.horizontal(|ui| {
                ui.label("Storage:");
                if ui.add(Slider::new(&mut storage_poll, 5.0..=60.0).step_by(0.5).suffix(" s")).changed() {
                    state.config.statistics_sections.storage_poll_rate = (storage_poll * 1000.0) as u64;
                    let _ = state.save_config();
                }
            });
            
            let mut fans_poll = (state.config.statistics_sections.fans_poll_rate as f32) / 1000.0;
            ui.horizontal(|ui| {
                ui.label("Fans:");
                if ui.add(Slider::new(&mut fans_poll, 0.5..=10.0).step_by(0.5).suffix(" s")).changed() {
                    state.config.statistics_sections.fans_poll_rate = (fans_poll * 1000.0) as u64;
                    let _ = state.save_config();
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
