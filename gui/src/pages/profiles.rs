use egui::{Ui, ScrollArea, RichText, Frame};
use crate::app::{AppState, Page};
use crate::dbus_client::DbusClient;

pub fn draw(ui: &mut Ui, state: &mut AppState, dbus_client: Option<&DbusClient>) {
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            
            ui.heading(format!("Current Profile: {}", state.config.current_profile));
            ui.add_space(12.0);
            
            // Profile list with radio buttons
            let mut profile_to_switch = None;
            let mut profile_to_delete = None;
            let mut profile_to_reset = None;
            
            for (idx, profile) in state.config.profiles.iter().enumerate() {
                let is_current = profile.name == state.config.current_profile;
                let is_standard = profile.name == "Standard";
                
                // Frame with highlight for current profile
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
                        // Radio button
                        if ui.radio(is_current, "").clicked() && !is_current {
                            profile_to_switch = Some(idx);
                        }
                        
                        // Profile name - clicking also selects
                        let name_text = if is_standard {
                            RichText::new(&profile.name).strong()
                        } else {
                            RichText::new(&profile.name)
                        };
                        
                        if ui.selectable_label(is_current, name_text).clicked() && !is_current {
                            profile_to_switch = Some(idx);
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Delete button (only for non-standard profiles)
                            if !is_standard {
                                if ui.button("🗑️ Delete").clicked() {
                                    profile_to_delete = Some(idx);
                                }
                            }
                            
                            // Reset to default button (only for standard profile)
                            if is_standard {
                                if ui.button("↺ Reset to Default").clicked() {
                                    profile_to_reset = Some(idx);
                                }
                            }
                            
                            // Edit button - switches to tuning page
                            if ui.button("✏️ Edit").clicked() {
                                if !is_current {
                                    profile_to_switch = Some(idx);
                                }
                                state.current_page = Page::Tuning;
                            }
                        });
                    });
                    
                    // Profile details summary
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        // CPU settings summary
                        if let Some(ref gov) = profile.cpu_settings.governor {
                            ui.label(RichText::new(format!("Governor: {}", gov)).small());
                            ui.label(RichText::new("|").small());
                        }
                        
                        if let Some(boost) = profile.cpu_settings.boost {
                            ui.label(RichText::new(format!("Boost: {}", if boost { "On" } else { "Off" })).small());
                            ui.label(RichText::new("|").small());
                        }
                        
                        // Keyboard settings
                        if profile.keyboard_settings.control_enabled {
                            ui.label(RichText::new("Keyboard: Manual").small());
                        } else {
                            ui.label(RichText::new("Keyboard: Auto").small());
                        }
                        
                        ui.label(RichText::new("|").small());
                        
                        // Fan settings
                        if profile.fan_settings.control_enabled {
                            ui.label(RichText::new(format!("Fans: Custom ({})", profile.fan_settings.curves.len())).small());
                        } else {
                            ui.label(RichText::new("Fans: Auto").small());
                        }
                    });
                });
                
                ui.add_space(8.0);
            }
            
            // Handle profile switch
            if let Some(idx) = profile_to_switch {
                state.config.current_profile = state.config.profiles[idx].name.clone();
                let _ = state.save_config();
                
                // Apply to hardware
                if let Some(client) = dbus_client {
                    let profile_clone = state.config.profiles[idx].clone();
                    let _rx = client.apply_profile(profile_clone);
                    state.show_message(format!("Switched to profile '{}'", state.config.profiles[idx].name), false);
                }
            }
            
            // Handle profile reset
            if let Some(idx) = profile_to_reset {
                state.config.profiles[idx] = create_standard_profile();
                let _ = state.save_config();
                
                // Apply if it's the current profile
                if state.config.profiles[idx].name == state.config.current_profile {
                    if let Some(client) = dbus_client {
                        let profile_clone = state.config.profiles[idx].clone();
                        let _rx = client.apply_profile(profile_clone);
                    }
                }
                state.show_message("Standard profile reset to default settings", false);
            }
            
            // Handle profile deletion
            if let Some(idx) = profile_to_delete {
                let name = state.config.profiles[idx].name.clone();
                
                // If deleting current profile, switch to Standard first
                if name == state.config.current_profile {
                    state.config.current_profile = "Standard".to_string();
                    if let Some(standard) = state.config.profiles.iter().find(|p| p.name == "Standard") {
                        if let Some(client) = dbus_client {
                            let _rx = client.apply_profile(standard.clone());
                        }
                    }
                }
                
                state.config.profiles.remove(idx);
                let _ = state.save_config();
                state.show_message(format!("Profile '{}' deleted", name), false);
            }
            
            // Add new profile section
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);
            
            ui.horizontal(|ui| {
                ui.label(RichText::new("Create New Profile:").strong());
                
                let text_edit_id = ui.make_persistent_id("new_profile_name");
                let mut new_name = state.editing_profile_name.clone().unwrap_or_default();
                ui.text_edit_singleline(&mut new_name);
                state.editing_profile_name = Some(new_name.clone());
                
                if ui.button("➕ Create").clicked() && !new_name.is_empty() {
                    if state.config.profiles.iter().any(|p| p.name == new_name) {
                        state.show_message(format!("Profile '{}' already exists", new_name), true);
                    } else {
                        // Create new profile based on current
                        let current_profile = state.current_profile()
                            .cloned()
                            .unwrap_or_else(create_standard_profile);
                        
                        let mut new_profile = current_profile;
                        new_profile.name = new_name.clone();
                        new_profile.is_default = false;
                        
                        state.config.profiles.push(new_profile);
                        state.editing_profile_name = None;
                        let _ = state.save_config();
                        state.show_message(format!("Profile '{}' created", new_name), false);
                    }
                }
            });
        });
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
        gpu_settings: GpuSettings { dgpu_tdp: None },
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
