use egui::{Ui, ScrollArea, RichText, Frame};
use crate::app::{AppState, Page};
use crate::dbus_client::DbusClient;

pub fn draw(ui: &mut Ui, state: &mut AppState, dbus_client: Option<&DbusClient>) {
    let (profile_to_switch, profile_to_delete, profile_to_reset, go_to_tuning, new_profile_name) = {
        let mut config = state.config.lock();
        let mut profile_to_switch = None;
        let mut profile_to_delete = None;
        let mut profile_to_reset = None;
        let mut go_to_tuning = false;
        let mut new_profile_name = state.editing_profile_name.clone();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                
                ui.heading(format!("Current Profile: {}", config.current_profile));
                ui.add_space(12.0);
                
                for (idx, profile) in config.profiles.iter().enumerate() {
                    let is_current = profile.name == config.current_profile;
                    let is_standard = profile.name == "Standard";

                    let frame = if is_current {
                        Frame::none()
                            .fill(ui.style().visuals.selection.bg_fill.gamma_multiply(0.3))
                            .stroke(ui.style().visuals.selection.stroke)
                            .rounding(6.0)
                            .inner_margin(12.0)
                    } else {
                        Frame::none()
                            .fill(ui.style().visuals.faint_bg_color)
                            .rounding(6.0)
                            .inner_margin(12.0)
                    };

                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.radio(is_current, "").clicked() && !is_current {
                                profile_to_switch = Some(idx);
                            }
                            
                            let name_text = if is_standard { RichText::new(&profile.name).strong() } else { RichText::new(&profile.name) };

                            if ui.selectable_label(is_current, name_text).clicked() && !is_current {
                                profile_to_switch = Some(idx);
                            }
                            
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if !is_standard {
                                    if ui.button("🗑️ Delete").clicked() { profile_to_delete = Some(idx); }
                                }
                                if is_standard {
                                    if ui.button("↺ Reset to Default").clicked() { profile_to_reset = Some(idx); }
                                }
                                if ui.button("✏️ Edit").clicked() {
                                    if !is_current { profile_to_switch = Some(idx); }
                                    go_to_tuning = true;
                                }
                            });
                        });
                        
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if let Some(ref gov) = profile.cpu_settings.governor {
                                ui.label(RichText::new(format!("Governor: {}", gov)).small());
                                ui.label(RichText::new("|").small());
                            }
                            if let Some(boost) = profile.cpu_settings.boost {
                                ui.label(RichText::new(format!("Boost: {}", if boost { "On" } else { "Off" })).small());
                                ui.label(RichText::new("|").small());
                            }
                            if profile.keyboard_settings.control_enabled {
                                ui.label(RichText::new("Keyboard: Manual").small());
                            } else {
                                ui.label(RichText::new("Keyboard: Auto").small());
                            }
                            ui.label(RichText::new("|").small());
                            if profile.fan_settings.control_enabled {
                                ui.label(RichText::new(format!("Fans: Custom ({})", profile.fan_settings.curves.len())).small());
                            } else {
                                ui.label(RichText::new("Fans: Auto").small());
                            }
                        });
                    });

                    ui.add_space(8.0);
                }
                
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Create New Profile:").strong());
                    let mut new_name_str = new_profile_name.clone().unwrap_or_default();
                    ui.text_edit_singleline(&mut new_name_str);
                    new_profile_name = Some(new_name_str);
                    if ui.button("➕ Create").clicked() {
                        // Logic moved outside the closure
                    }
                });
            });

        (profile_to_switch, profile_to_delete, profile_to_reset, go_to_tuning, new_profile_name)
    };

    if go_to_tuning { state.current_page = Page::Tuning; }

    if let Some(idx) = profile_to_switch {
        let (profile_name, profile_clone) = {
            let mut config = state.config.lock();
            let profile_name = config.profiles[idx].name.clone();
            config.current_profile = profile_name.clone();
            (profile_name, config.profiles[idx].clone())
        };
        if let Some(client) = dbus_client {
            let _rx = client.apply_profile(profile_clone);
            state.show_message(format!("Switched to profile '{}'", profile_name), false);
        }
    }

    if let Some(idx) = profile_to_reset {
        let (profile_name, current_profile, profile_clone) = {
            let mut config = state.config.lock();
            config.profiles[idx] = create_standard_profile();
            (config.profiles[idx].name.clone(), config.current_profile.clone(), config.profiles[idx].clone())
        };
        if profile_name == current_profile {
            if let Some(client) = dbus_client {
                let _rx = client.apply_profile(profile_clone);
            }
        }
        state.show_message("Standard profile reset to default settings", false);
    }

    if let Some(idx) = profile_to_delete {
        let name = {
            let mut config = state.config.lock();
            let name = config.profiles[idx].name.clone();
            if name == config.current_profile {
                config.current_profile = "Standard".to_string();
                if let Some(standard) = config.profiles.iter().find(|p| p.name == "Standard") {
                    if let Some(client) = dbus_client {
                        let standard = standard.clone();
                        drop(config);
                        let _rx = client.apply_profile(standard);
                        config = state.config.lock();
                    }
                }
            }
            config.profiles.remove(idx);
            name
        };
        state.show_message(format!("Profile '{}' deleted", name), false);
    }

    if let Some(new_name) = new_profile_name {
        if !new_name.is_empty() {
            let mut config = state.config.lock();
            if config.profiles.iter().any(|p| p.name == new_name) {
                drop(config);
                state.show_message(format!("Profile '{}' already exists", new_name), true);
            } else {
                let current_profile = config.profiles.iter().find(|p| p.name == config.current_profile).cloned().unwrap_or_else(create_standard_profile);
                let mut new_profile = current_profile;
                new_profile.name = new_name.clone();
                new_profile.is_default = false;
                config.profiles.push(new_profile);
                state.editing_profile_name = None;
                drop(config);
                state.show_message(format!("Profile '{}' created", new_name), false);
            }
        }
    }
}

fn create_standard_profile() -> tuxedo_common::types::Profile {
    use tuxedo_common::types::*;
    
    Profile {
        name: "Standard".to_string(),
        is_default: true,
        cpu_settings: CpuSettings {
            governor: Some("schedutil".to_string()),
            min_frequency: None,
            max_frequency: None,
            boost: Some(true),
            smt: Some(true),
            performance_profile: None,
            tdp_profile: None,
            energy_performance_preference: Some("balance_performance".to_string()),
            tdp: None,
            amd_pstate_status: Some("active".to_string()),
        },
        gpu_settings: GpuSettings {
            dgpu_tdp: None,
            min_gpu_clock: None,
            max_gpu_clock: None,
            min_mem_clock: None,
            max_mem_clock: None,
            manual_clocks: false,
            core_offset: Some(0),
            memory_offset: Some(0),
            prime_profile: Some("on-demand".to_string()),
        },
        keyboard_settings: KeyboardSettings {
            control_enabled: false,
            mode: KeyboardMode::SingleColor {
                r: 255,
                g: 255,
                b: 255,
                brightness: 50,
            },
        },
        screen_settings: ScreenSettings {
            brightness: 50,
            system_control: true,
        },
        fan_settings: FanSettings {
            control_enabled: false,
            curves: vec![],
        },
    }
}
