use egui::{Ui, ScrollArea, RichText, Slider, ComboBox, TopBottomPanel, Grid};
use crate::app::AppState;
use crate::dbus_client::DbusClient;
use lapsphere_common::types::{KeyboardMode, Profile, FanCurve, KeyboardCapabilities, FanInfo};
use crate::widgets::fan_curve_editor::FanCurveEditor;

pub fn draw(ui: &mut Ui, state: &mut AppState, dbus_client: Option<&DbusClient>, hw_update_tx: tokio::sync::mpsc::UnboundedSender<crate::app::HardwareUpdate>) {
    let profile_idx = state.current_profile_index();
    
    if profile_idx.is_none() {
        ui.label("No profile selected");
        return;
    }
    
    let idx = profile_idx.unwrap();
    let profile_name = state.config.profiles[idx].name.clone();
    let is_standard = profile_name == "Standard";
    
    // Top bar with profile name, save, and reset buttons
    TopBottomPanel::top("tuning_header").show_inside(ui, |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading(format!("Editing: {}", profile_name));
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Save button - always visible
                if ui.button("💾 Save").clicked() {
                    let _ = state.save_profiles();
                    
                    // Also apply to hardware
                    if let Some(client) = dbus_client {
                        let profile_clone = state.config.profiles[idx].clone();
                        let _rx = client.apply_profile(profile_clone.clone());
                        
                        // Apply GPU settings on save
                        apply_gpu_settings_on_save(client, &profile_clone.gpu_settings);
                    }
                }
                
                // Reset to default button
                if ui.button("↺ Reset to Default").clicked() {
                    state.config.profiles[idx] = create_default_profile_for_reset(is_standard);
                    state.show_message("Profile reset to default settings (not saved)", false);
                }
            });
        });
        ui.add_space(8.0);
    });
    
    // Main content
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            
            let cpu_info_clone = state.cpu_info.clone();

            // Performance Profile (Separate section)
            let tdp_profiles = state.available_tdp_profiles.clone();
            if !tdp_profiles.is_empty() {
                ui.heading("🚀 Performance Profile");
                ui.add_space(6.0);
                draw_performance_profile_tuning(ui, &mut state.config.profiles[idx], &tdp_profiles);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
            }

            // CPU tuning
            if let Some(cpu_info) = &cpu_info_clone {
                let cpu_caps = Some(&cpu_info.capabilities);
                draw_cpu_tuning(
                    ui,
                    &mut state.config.profiles[idx],
                    cpu_caps,
                    cpu_info,
                    dbus_client,
                    hw_update_tx.clone(),
                );
            } else {
                ui.heading("🖥 CPU Tuning");
                ui.add_space(6.0);
                ui.label("CPU information not available");
            }
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            
            // GPU tuning
            let gpu_info = state.gpu_info.clone();
            draw_gpu_tuning(ui, idx, dbus_client, &gpu_info, state, hw_update_tx.clone());
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);

            // Keyboard tuning
            let keyboard_caps = state.keyboard_capabilities.clone();
            draw_keyboard_tuning(ui, &mut state.config.profiles[idx], keyboard_caps.as_ref(), dbus_client);
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            
            // Screen tuning
            draw_screen_tuning(ui, &mut state.config.profiles[idx]);
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            
            // Fan tuning
            let fan_count = state.fan_info.len();
            let fan_info = state.fan_info.clone();
            let mut selected_fan_curve = state.selected_fan_curve;
            draw_fan_tuning(
                ui,
                &mut state.config.profiles[idx],
                &fan_info,
                &mut selected_fan_curve,
                fan_count,
            );
            state.selected_fan_curve = selected_fan_curve;
            ui.add_space(12.0);
        });
}

fn draw_performance_profile_tuning(
    ui: &mut Ui,
    profile: &mut Profile,
    tdp_profiles: &[String],
) {
    ui.horizontal(|ui| {
        let mut current_profile = profile.cpu_settings.tdp_profile.clone();
        let selected_label = current_profile
            .clone()
            .unwrap_or_else(|| "System default".to_string());

        ComboBox::from_id_salt("performance_profile_combo")
            .selected_text(&selected_label)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current_profile.is_none(), "System default").clicked() {
                    current_profile = None;
                }
                for profile_name in tdp_profiles {
                    if ui.selectable_label(
                        current_profile.as_deref() == Some(profile_name.as_str()),
                        profile_name,
                    ).clicked() {
                        current_profile = Some(profile_name.clone());
                    }
                }
            });

        profile.cpu_settings.tdp_profile = current_profile;
    });
}

fn draw_cpu_tuning(
    ui: &mut Ui,
    profile: &mut Profile,
    cpu_caps: Option<&lapsphere_common::types::CpuCapabilities>,
    cpu_info: &lapsphere_common::types::CpuInfo,
    dbus_client: Option<&DbusClient>,
    hw_update_tx: tokio::sync::mpsc::UnboundedSender<crate::app::HardwareUpdate>,
) {
    ui.heading("🖥 CPU Tuning");
    ui.add_space(8.0);
    
    let caps = match cpu_caps {
        Some(c) => c,
        None => {
            ui.label("CPU information not available");
            return;
        }
    };

    // AMD P-State section (if available)
    if caps.has_amd_pstate {
        ui.label(RichText::new("AMD P-State Mode:").strong());
        ui.horizontal(|ui| {
            let mut current_pstate = profile.cpu_settings.amd_pstate_status
                .clone()
                .unwrap_or_else(|| "active".to_string());
            let previous_pstate = current_pstate.clone();
            
            ComboBox::from_id_salt("amd_pstate_combo")
                .selected_text(&current_pstate)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut current_pstate, "active".to_string(), "Active");
                    ui.selectable_value(&mut current_pstate, "passive".to_string(), "Passive");
                    ui.selectable_value(&mut current_pstate, "guided".to_string(), "Guided");
                });

            if current_pstate != previous_pstate {
                if let Some(client) = dbus_client {
                    let client = client.clone();
                    let tx = hw_update_tx.clone();
                    let pstate = current_pstate.clone();
                    tokio::spawn(async move {
                        let _ = client.set_amd_pstate_status(pstate).await;
                        if let Ok(result) = client.get_cpu_info().await {
                            if let Ok(info) = result {
                                let _ = tx.send(crate::app::HardwareUpdate::CpuInfo(info));
                            }
                        }
                    });
                }
            }

            profile.cpu_settings.amd_pstate_status = Some(current_pstate);
            
            ui.label(RichText::new("(Active = best performance, Passive = better efficiency)")
                .small()
                .italics());
        });
        ui.add_space(6.0);
    }

    // Intel P-State section (if available)
    if caps.has_intel_pstate {
        ui.label(RichText::new("Intel P-State Mode:").strong());
        ui.horizontal(|ui| {
            let mut current_pstate = profile.cpu_settings.intel_pstate_status
                .clone()
                .unwrap_or_else(|| "active".to_string());
            let previous_pstate = current_pstate.clone();

            ComboBox::from_id_salt("intel_pstate_combo")
                .selected_text(&current_pstate)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut current_pstate, "active".to_string(), "Active");
                    ui.selectable_value(&mut current_pstate, "passive".to_string(), "Passive");
                });

            if current_pstate != previous_pstate {
                if let Some(client) = dbus_client {
                    let client = client.clone();
                    let tx = hw_update_tx.clone();
                    let pstate = current_pstate.clone();
                    tokio::spawn(async move {
                        let _ = client.set_intel_pstate_status(pstate).await;
                        if let Ok(result) = client.get_cpu_info().await {
                            if let Ok(info) = result {
                                let _ = tx.send(crate::app::HardwareUpdate::CpuInfo(info));
                            }
                        }
                    });
                }
            }

            profile.cpu_settings.intel_pstate_status = Some(current_pstate);
        });
        ui.add_space(6.0);
    }
    
    // Governor
    if caps.has_scaling_governor && !cpu_info.available_governors.is_empty() {
        ui.label(RichText::new("Governor:").strong());
        ui.horizontal(|ui| {
            let mut current_gov = profile.cpu_settings.governor
                .clone()
                .unwrap_or_else(|| {
                    // Use first available governor as default
                    cpu_info.available_governors.first()
                        .cloned()
                        .unwrap_or_else(|| "performance".to_string())
                });
            
            ComboBox::from_id_salt("governor_combo")
                .selected_text(&current_gov)
                .show_ui(ui, |ui| {
                    for gov in &cpu_info.available_governors {
                        ui.selectable_value(&mut current_gov, gov.clone(), gov);
                    }
                });
            
            profile.cpu_settings.governor = Some(current_gov);
        });
        ui.add_space(6.0);
    }
    
    // EPP
    if caps.has_energy_performance_preference && !cpu_info.available_epp_options.is_empty() {
        ui.label(RichText::new("Energy Performance Preference:").strong());
        ui.horizontal(|ui| {
            let mut current_epp = profile.cpu_settings.energy_performance_preference
                .clone()
                .unwrap_or_else(|| "balance_performance".to_string());
            
            ComboBox::from_id_salt("epp_combo")
                .selected_text(&current_epp)
                .show_ui(ui, |ui| {
                    for epp in &cpu_info.available_epp_options {
                        ui.selectable_value(&mut current_epp, epp.clone(), epp);
                    }
                });
            
            profile.cpu_settings.energy_performance_preference = Some(current_epp);
        });
        ui.add_space(6.0);
    }

    // Frequency sliders
    if caps.has_scaling_min_freq && caps.has_scaling_max_freq {
        ui.label(RichText::new("Frequency Limits:").strong());

        if let (Some(hw_min), Some(hw_max)) = (cpu_info.hw_min_freq, cpu_info.hw_max_freq) {
            let mut min_freq = profile.cpu_settings.min_frequency
                .unwrap_or(hw_min) as f64 / 1000.0;
            let mut max_freq = profile.cpu_settings.max_frequency
                .unwrap_or(hw_max) as f64 / 1000.0;

            // Ensure min <= max
            if min_freq > max_freq {
                min_freq = max_freq;
            }

            ui.horizontal(|ui| {
                ui.label("Min:");
                if ui.add(Slider::new(&mut min_freq,
                    (hw_min / 1000) as f64..=(hw_max / 1000) as f64)
                    .suffix(" MHz")).changed() {
                    // Ensure min doesn't exceed max
                    if min_freq > max_freq {
                        max_freq = min_freq;
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Max:");
                if ui.add(Slider::new(&mut max_freq,
                    (hw_min / 1000) as f64..=(hw_max / 1000) as f64)
                    .suffix(" MHz")).changed() {
                    // Ensure max doesn't go below min
                    if max_freq < min_freq {
                        min_freq = max_freq;
                    }
                }
            });

            profile.cpu_settings.min_frequency = Some((min_freq * 1000.0) as u64);
            profile.cpu_settings.max_frequency = Some((max_freq * 1000.0) as u64);

        } else {
            ui.label("Could not determine hardware frequency limits. Sliders disabled.");
        }

        ui.add_space(6.0);
    }
    
    // Boost checkbox
    if caps.has_boost {
        let mut boost = profile.cpu_settings.boost.unwrap_or(true);
        ui.checkbox(&mut boost, "CPU Boost / Turbo");
        profile.cpu_settings.boost = Some(boost);
        
        // Show if boost is available for current pstate
        if caps.has_amd_pstate {
            ui.label(RichText::new("(Available in all AMD P-State modes)")
                .small()
                .italics());
        }
    }
    
    // SMT checkbox
    if caps.has_smt {
        let mut smt = profile.cpu_settings.smt.unwrap_or(true);
        ui.checkbox(&mut smt, "SMT / Hyperthreading");
        profile.cpu_settings.smt = Some(smt);
    }
}

fn draw_gpu_tuning(
    ui: &mut Ui,
    profile_idx: usize,
    dbus_client: Option<&DbusClient>,
    gpu_info: &[lapsphere_common::types::GpuInfo],
    state: &mut AppState,
    hw_update_tx: tokio::sync::mpsc::UnboundedSender<crate::app::HardwareUpdate>,
) {
    ui.heading("GPU Tuning");
    ui.add_space(8.0);

    let is_nvidia = gpu_info.iter().any(|g| g.name.contains("NVIDIA"));

    if !is_nvidia {
        ui.label("NVIDIA GPU not detected. Overclocking is only available for NVIDIA GPUs.");
        return;
    }

    let mut manual_clocks = state.config.profiles[profile_idx].gpu_settings.manual_clocks;
    ui.checkbox(&mut manual_clocks, "Enable Manual Clock Control");
    state.config.profiles[profile_idx].gpu_settings.manual_clocks = manual_clocks;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Mode:");
        ui.radio_value(&mut state.config.profiles[profile_idx].gpu_settings.advanced_control, false, "Standard");
        ui.radio_value(&mut state.config.profiles[profile_idx].gpu_settings.advanced_control, true, "Advanced");
    });
    let tuning_mode_advanced = state.config.profiles[profile_idx].gpu_settings.advanced_control;
    ui.label(RichText::new("GPU settings will be applied when you click Save. Disabling manual control will reset to factory settings.").small().italics());

    if state.config.profiles[profile_idx].gpu_settings.manual_clocks {
        // Fetch ranges if they haven't been fetched yet
        if state.gpu_clock_ranges.is_none() {
            if let Some(client) = dbus_client {
                let client = client.clone();
                let tx = hw_update_tx.clone();
                tokio::spawn(async move {
                    let res = client.get_gpu_clock_ranges(0).await
                        .map(|r| r.map_err(|e| e.to_string()))
                        .unwrap_or_else(|e| Err(e.to_string()));
                    let _ = tx.send(crate::app::HardwareUpdate::GpuClockRanges(res));
                });
            }
        }
        if state.gpu_mem_clock_ranges.is_none() {
            if let Some(client) = dbus_client {
                let client = client.clone();
                let tx = hw_update_tx.clone();
                tokio::spawn(async move {
                    let res = client.get_gpu_mem_clock_ranges(0).await
                        .map(|r| r.map_err(|e| e.to_string()))
                        .unwrap_or_else(|e| Err(e.to_string()));
                    let _ = tx.send(crate::app::HardwareUpdate::GpuMemClockRanges(res));
                });
            }
        }
        if state.gpu_core_offset_limits.is_none() {
            if let Some(client) = dbus_client {
                let client = client.clone();
                let tx = hw_update_tx.clone();
                tokio::spawn(async move {
                    let res = client.get_gpu_core_offset_limits(0).await
                        .map(|r| r.map_err(|e| e.to_string()))
                        .unwrap_or_else(|e| Err(e.to_string()));
                    let _ = tx.send(crate::app::HardwareUpdate::GpuCoreOffsetLimits(res));
                });
            }
        }
        if state.gpu_mem_offset_limits.is_none() {
            if let Some(client) = dbus_client {
                let client = client.clone();
                let tx = hw_update_tx.clone();
                tokio::spawn(async move {
                    let res = client.get_gpu_memory_offset_limits(0).await
                        .map(|r| r.map_err(|e| e.to_string()))
                        .unwrap_or_else(|e| Err(e.to_string()));
                    let _ = tx.send(crate::app::HardwareUpdate::GpuMemOffsetLimits(res));
                });
            }
        }

    if state.config.profiles[profile_idx].gpu_settings.manual_clocks {
        let profile = &mut state.config.profiles[profile_idx];
        draw_gpu_standard_controls(
            ui,
            profile,
            state.gpu_clock_ranges,
            state.gpu_mem_clock_ranges,
            state.gpu_core_offset_limits,
            state.gpu_mem_offset_limits,
        );
        if tuning_mode_advanced {
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);
            draw_gpu_advanced_controls(ui, profile);
        }

        ui.add_space(16.0);
        ui.label(RichText::new("Clock and offset controls are applied when you click Save. They are managed by NVML.").small().italics());
    }}
}

fn draw_gpu_standard_controls(
    ui: &mut Ui,
    profile: &mut Profile,
    gpu_clock_ranges: Option<(u32, u32)>,
    gpu_mem_clock_ranges: Option<(u32, u32)>,
    gpu_core_offset_limits: Option<(i32, i32)>,
    gpu_mem_offset_limits: Option<(i32, i32)>,
) {
    // GPU Locked Clocks section
    ui.label(RichText::new("GPU Locked Clocks:").strong());
    if let Some((min_range, max_range)) = gpu_clock_ranges {
        let mut min_gpu_clock = profile.gpu_settings.min_gpu_clock.unwrap_or(min_range);
        let mut max_gpu_clock = profile.gpu_settings.max_gpu_clock.unwrap_or(max_range);

        ui.horizontal(|ui| {
            ui.label("Min:");
            ui.add(Slider::new(&mut min_gpu_clock, min_range..=max_range).suffix(" MHz"));
        });

        ui.horizontal(|ui| {
            ui.label("Max:");
            ui.add(Slider::new(&mut max_gpu_clock, min_range..=max_range).suffix(" MHz"));
        });

        profile.gpu_settings.min_gpu_clock = Some(min_gpu_clock);
        profile.gpu_settings.max_gpu_clock = Some(max_gpu_clock);
    } else {
        ui.label("Fetching GPU clock ranges...");
    }

    ui.add_space(16.0);

    // Memory Locked Clocks section
    ui.label(RichText::new("Memory Locked Clocks:").strong());
    if let Some((min_range, max_range)) = gpu_mem_clock_ranges {
        let mut min_mem_clock = profile.gpu_settings.min_mem_clock.unwrap_or(min_range);
        let mut max_mem_clock = profile.gpu_settings.max_mem_clock.unwrap_or(max_range);

        ui.horizontal(|ui| {
            ui.label("Min:");
            ui.add(Slider::new(&mut min_mem_clock, min_range..=max_range).suffix(" MHz"));
        });

        ui.horizontal(|ui| {
            ui.label("Max:");
            ui.add(Slider::new(&mut max_mem_clock, min_range..=max_range).suffix(" MHz"));
        });

        profile.gpu_settings.min_mem_clock = Some(min_mem_clock);
        profile.gpu_settings.max_mem_clock = Some(max_mem_clock);
    } else {
        ui.label("Fetching memory clock ranges...");
    }

    ui.add_space(16.0);

    // GPU Core Offset - only for Standard control
    if !profile.gpu_settings.advanced_control {
        ui.label(RichText::new("GPU Core Offset:").strong());
        if let Some((min_limit, max_limit)) = gpu_core_offset_limits {
            let mut core_offset = profile.gpu_settings.core_offset.unwrap_or(0);
            ui.add(Slider::new(&mut core_offset, min_limit..=max_limit).suffix(" MHz"));
            profile.gpu_settings.core_offset = Some(core_offset);
        }

        ui.add_space(16.0);
    }

    // GPU Memory Offset
    ui.label(RichText::new("GPU Memory Offset:").strong());
    if let Some((min_limit, max_limit)) = gpu_mem_offset_limits {
        let mut memory_offset = profile.gpu_settings.memory_offset.unwrap_or(0);
        ui.add(Slider::new(&mut memory_offset, min_limit..=max_limit).suffix(" MHz"));
        profile.gpu_settings.memory_offset = Some(memory_offset);
    } else {
        ui.label("Fetching GPU memory offset limits...");
    }
}

fn draw_gpu_advanced_controls(ui: &mut Ui, profile: &mut Profile) {
    ui.label(RichText::new("Advanced GPU Tuning (Dynamic Offsets):").strong());
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.checkbox(&mut profile.gpu_settings.advanced.drain_offset_control, "Drain Offset Control");
        ui.checkbox(&mut profile.gpu_settings.advanced.power_offset_control, "Power Offset Control");
    });
    ui.checkbox(&mut profile.gpu_settings.advanced.critical_temp_range_control, "Critical Temperature Range Control");

    ui.horizontal(|ui| {
        ui.label("Smart Rounding Threshold:");
        ui.add(egui::DragValue::new(&mut profile.gpu_settings.advanced.smart_rounding_threshold).speed(1).range(0..=100));
        ui.label(RichText::new("(Applied at P-State 0)").small().italics());
    });

    ui.add_space(8.0);

    Grid::new("gpu_advanced_grid")
        .num_columns(3)
        .spacing([12.0, 8.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("Setting").strong());
            ui.label(RichText::new("Min").strong());
            ui.label(RichText::new("Max").strong());
            ui.end_row();

            add_range_row(
                ui,
                "Temperature (°C)",
                &mut profile.gpu_settings.advanced.temperature_min,
                &mut profile.gpu_settings.advanced.temperature_max,
            );
            add_range_row(
                ui,
                "Power limits (W)",
                &mut profile.gpu_settings.advanced.plimit_min,
                &mut profile.gpu_settings.advanced.plimit_max,
            );
            add_range_row(
                ui,
                "Frequency thresholds (MHz)",
                &mut profile.gpu_settings.advanced.frequency_min,
                &mut profile.gpu_settings.advanced.frequency_max,
            );
            add_range_row(
                ui,
                "Base frequency offsets (MHz)",
                &mut profile.gpu_settings.advanced.freq_offset_min,
                &mut profile.gpu_settings.advanced.freq_offset_max,
            );
            add_range_row(
                ui,
                "Low frequency range (MHz)",
                &mut profile.gpu_settings.advanced.low_freq_min,
                &mut profile.gpu_settings.advanced.low_freq_max,
            );
            add_range_row(
                ui,
                "Low drain offsets (MHz)",
                &mut profile.gpu_settings.advanced.drain_offset_lmin,
                &mut profile.gpu_settings.advanced.drain_offset_lmax,
            );
            add_range_row(
                ui,
                "High frequency range (MHz)",
                &mut profile.gpu_settings.advanced.high_freq_min,
                &mut profile.gpu_settings.advanced.high_freq_max,
            );
            add_range_row(
                ui,
                "High drain offsets (MHz)",
                &mut profile.gpu_settings.advanced.drain_offset_hmin,
                &mut profile.gpu_settings.advanced.drain_offset_hmax,
            );
            add_range_row(
                ui,
                "Critical temperature (°C)",
                &mut profile.gpu_settings.advanced.critical_temp_min,
                &mut profile.gpu_settings.advanced.critical_temp_max,
            );
            add_range_row(
                ui,
                "Power offset range (MHz)",
                &mut profile.gpu_settings.advanced.power_offset_min,
                &mut profile.gpu_settings.advanced.power_offset_max,
            );
        });

    ui.add_space(6.0);
    ui.label(RichText::new("Values are examples. Set what is appropriate for your hardware.").small().italics());
}

fn add_range_row(ui: &mut Ui, label: &str, min_value: &mut i32, max_value: &mut i32) {
    ui.label(label);
    ui.add(egui::DragValue::new(min_value).speed(1));
    ui.add(egui::DragValue::new(max_value).speed(1));
    ui.end_row();
}

/// Apply GPU settings when the save button is clicked
fn apply_gpu_settings_on_save(client: &DbusClient, gpu_settings: &lapsphere_common::types::GpuSettings) {
    if gpu_settings.manual_clocks {
        // Apply GPU locked clocks if set
        if let (Some(min_clock), Some(max_clock)) = (gpu_settings.min_gpu_clock, gpu_settings.max_gpu_clock) {
            let _ = client.set_gpu_locked_clocks(0, min_clock, max_clock);
        }
        
        // Apply memory locked clocks if set
        if let (Some(min_mem), Some(max_mem)) = (gpu_settings.min_mem_clock, gpu_settings.max_mem_clock) {
            let _ = client.set_memory_locked_clocks(0, min_mem, max_mem);
        }
        
        // Apply core offset if set and not in advanced mode (advanced mode is handled by daemon loop)
        if !gpu_settings.advanced_control {
            if let Some(core_offset) = gpu_settings.core_offset {
                let _ = client.set_gpu_core_offset(0, core_offset);
            }
        }
        
        // Apply memory offset if set
        if let Some(memory_offset) = gpu_settings.memory_offset {
            let _ = client.set_gpu_memory_offset(0, memory_offset);
        }
    } else {
        // Reset to factory settings when manual control is disabled
        let _ = client.reset_gpu_clocks(0);
        let _ = client.reset_memory_locked_clocks(0);
        // Reset offsets to 0
        let _ = client.set_gpu_core_offset(0, 0);
        let _ = client.set_gpu_memory_offset(0, 0);
    }
}

fn draw_keyboard_tuning(
    ui: &mut Ui,
    profile: &mut Profile,
    caps: Option<&KeyboardCapabilities>,
    dbus_client: Option<&DbusClient>,
) {
    ui.heading("⌨ Keyboard Backlight");
    ui.add_space(8.0);
    
    let caps = match caps {
        Some(c) => c,
        None => {
            ui.label("Keyboard capabilities not available");
            return;
        }
    };

    let keyboard_detected = caps.keyboard_type != lapsphere_common::types::KeyboardType::None;
    if ui.checkbox(&mut profile.keyboard_settings.control_enabled, "Control keyboard backlight").changed() {
        if !profile.keyboard_settings.control_enabled {
            // Set to white when disabling control
            profile.keyboard_settings.mode = lapsphere_common::types::KeyboardMode::SingleColor {
                r: 255,
                g: 255,
                b: 255,
                brightness: 50,
            };
        }
    }
    ui.add_space(6.0);
    
    if profile.keyboard_settings.control_enabled {
        if !keyboard_detected {
            ui.label(RichText::new("Keyboard backlight device not detected.").small());
        } else {
            ui.label(RichText::new(format!("Detected: {:?}", caps.keyboard_type)).small());
            if !caps.supports_effects {
                ui.label(RichText::new("Effect support was not detected; some modes may not apply.").small());
            }
            if !caps.supports_color {
                ui.label(RichText::new("RGB color control is not available on this keyboard.").small());
            }
        }

        // Mode selector
        ui.horizontal(|ui| {
            ui.label("Mode:");
            
            let current_mode_name = match &profile.keyboard_settings.mode {
                KeyboardMode::SingleColor { .. } => "Single Color",
                KeyboardMode::MultipleZones { .. } => "Multiple Zones",
                KeyboardMode::Breathe { .. } => "Breathe",
                KeyboardMode::Cycle { .. } => "Cycle",
                KeyboardMode::Dance { .. } => "Dance",
                KeyboardMode::Flash { .. } => "Flash",
                KeyboardMode::RandomColor { .. } => "Random Color",
                KeyboardMode::Tempo { .. } => "Tempo",
                KeyboardMode::Wave { .. } => "Wave",
            };
            
            ComboBox::from_id_salt("keyboard_mode")
                .selected_text(current_mode_name)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current_mode_name == "Single Color", "Single Color").clicked() {
                        profile.keyboard_settings.mode = KeyboardMode::SingleColor { r: 255, g: 255, b: 255, brightness: 50 };
                    }
                    if caps.num_zones > 1 {
                        if ui.selectable_label(current_mode_name == "Multiple Zones", "Multiple Zones").clicked() {
                            let mut zones = Vec::new();
                            for _ in 0..caps.num_zones {
                                zones.push(lapsphere_common::types::ZoneColor { r: 255, g: 255, b: 255 });
                            }
                            profile.keyboard_settings.mode = KeyboardMode::MultipleZones { zones, brightness: 50 };
                        }
                    }
                    if caps.supports_effects {
                        if ui.selectable_label(current_mode_name == "Breathe", "Breathe").clicked() {
                            profile.keyboard_settings.mode = KeyboardMode::Breathe { r: 255, g: 255, b: 255, brightness: 50, speed: 50 };
                        }
                        if ui.selectable_label(current_mode_name == "Cycle", "Cycle").clicked() {
                            profile.keyboard_settings.mode = KeyboardMode::Cycle { brightness: 50, speed: 50 };
                        }
                        if ui.selectable_label(current_mode_name == "Dance", "Dance").clicked() {
                            profile.keyboard_settings.mode = KeyboardMode::Dance { brightness: 50, speed: 50 };
                        }
                        if ui.selectable_label(current_mode_name == "Flash", "Flash").clicked() {
                            profile.keyboard_settings.mode = KeyboardMode::Flash { r: 255, g: 255, b: 255, brightness: 50, speed: 50 };
                        }
                        if ui.selectable_label(current_mode_name == "Random Color", "Random Color").clicked() {
                            profile.keyboard_settings.mode = KeyboardMode::RandomColor { brightness: 50, speed: 50 };
                        }
                        if ui.selectable_label(current_mode_name == "Tempo", "Tempo").clicked() {
                            profile.keyboard_settings.mode = KeyboardMode::Tempo { brightness: 50, speed: 50 };
                        }
                        if ui.selectable_label(current_mode_name == "Wave", "Wave").clicked() {
                            profile.keyboard_settings.mode = KeyboardMode::Wave { brightness: 50, speed: 50 };
                        }
                    }
                });
        });
        ui.add_space(6.0);
        
        // Mode-specific controls
        match &mut profile.keyboard_settings.mode {
            KeyboardMode::SingleColor { r, g, b, brightness } => {
                if caps.supports_color {
                    ui.horizontal(|ui| {
                        ui.label("Red:");
                        ui.add(Slider::new(r, 0..=255));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Green:");
                        ui.add(Slider::new(g, 0..=255));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Blue:");
                        ui.add(Slider::new(b, 0..=255));
                    });

                    // Color preview
                    let color = egui::Color32::from_rgb(*r, *g, *b);
                    ui.horizontal(|ui| {
                        ui.label("Preview:");
                        ui.colored_label(color, "■■■■■");
                    });
                }
                
                if caps.supports_brightness {
                    ui.horizontal(|ui| {
                        ui.label("Brightness:");
                        ui.add(Slider::new(brightness, 0..=100).suffix("%"));
                    });
                }
            }
            KeyboardMode::MultipleZones { zones, brightness } => {
                for (i, zone) in zones.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.label(format!("Zone {}", i));
                        ui.horizontal(|ui| {
                            ui.label("R:");
                            ui.add(Slider::new(&mut zone.r, 0..=255));
                            ui.label("G:");
                            ui.add(Slider::new(&mut zone.g, 0..=255));
                            ui.label("B:");
                            ui.add(Slider::new(&mut zone.b, 0..=255));

                            let color = egui::Color32::from_rgb(zone.r, zone.g, zone.b);
                            ui.colored_label(color, "■");
                        });
                    });
                }

                if caps.supports_brightness {
                    ui.horizontal(|ui| {
                        ui.label("Overall Brightness:");
                        ui.add(Slider::new(brightness, 0..=100).suffix("%"));
                    });
                }
            }
            KeyboardMode::Breathe { r, g, b, brightness, speed } => {
                if caps.supports_color {
                    ui.horizontal(|ui| {
                        ui.label("Red:");
                        ui.add(Slider::new(r, 0..=255));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Green:");
                        ui.add(Slider::new(g, 0..=255));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Blue:");
                        ui.add(Slider::new(b, 0..=255));
                    });
                }
                if caps.supports_brightness {
                    ui.horizontal(|ui| {
                        ui.label("Brightness:");
                        ui.add(Slider::new(brightness, 0..=100).suffix("%"));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Speed:");
                    ui.add(Slider::new(speed, 0..=100).suffix("%"));
                });
            }
            KeyboardMode::Flash { r, g, b, brightness, speed } => {
                if caps.supports_color {
                    ui.horizontal(|ui| {
                        ui.label("Red:");
                        ui.add(Slider::new(r, 0..=255));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Green:");
                        ui.add(Slider::new(g, 0..=255));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Blue:");
                        ui.add(Slider::new(b, 0..=255));
                    });
                }
                if caps.supports_brightness {
                    ui.horizontal(|ui| {
                        ui.label("Brightness:");
                        ui.add(Slider::new(brightness, 0..=100).suffix("%"));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Speed:");
                    ui.add(Slider::new(speed, 0..=100).suffix("%"));
                });
            }
            KeyboardMode::Cycle { brightness, speed }
            | KeyboardMode::Dance { brightness, speed }
            | KeyboardMode::RandomColor { brightness, speed }
            | KeyboardMode::Tempo { brightness, speed }
            | KeyboardMode::Wave { brightness, speed } => {
                if caps.supports_brightness {
                    ui.horizontal(|ui| {
                        ui.label("Brightness:");
                        ui.add(Slider::new(brightness, 0..=100).suffix("%"));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Speed:");
                    ui.add(Slider::new(speed, 0..=100).suffix("%"));
                });
            }
        }
        
        // Preview button
        if ui.button("👁 Preview").clicked() {
            if let Some(client) = dbus_client {
                let _ = client.preview_keyboard_settings(profile.keyboard_settings.clone());
            }
        }
    }
}

fn draw_screen_tuning(ui: &mut Ui, profile: &mut Profile) {
    ui.heading("🖥 Screen");
    ui.add_space(8.0);
    
    ui.checkbox(&mut profile.screen_settings.system_control, "Use system brightness control");
    ui.add_space(6.0);
    
    if !profile.screen_settings.system_control {
        ui.horizontal(|ui| {
            ui.label("Brightness:");
            ui.add(Slider::new(&mut profile.screen_settings.brightness, 0..=100).suffix("%"));
        });
    }
}

fn draw_fan_tuning(
    ui: &mut Ui,
    profile: &mut Profile,
    fan_info: &[FanInfo],
    selected_fan_curve: &mut usize,
    fan_count: usize,
) {
    ui.heading("💨 Fan Control");
    ui.add_space(8.0);
    
    ui.checkbox(&mut profile.fan_settings.control_enabled, "Enable custom fan curves");
    ui.add_space(6.0);
    
    if profile.fan_settings.control_enabled {
        if fan_count == 0 {
            ui.label("No fans detected");
            return;
        }
        // Ensure curves exist
        while profile.fan_settings.curves.len() < fan_count {
            let fan_id = profile.fan_settings.curves.len() as u32;
            profile.fan_settings.curves.push(FanCurve {
                fan_id,
                points: vec![(0, 0), (50, 50), (70, 75), (85, 100)],
            });
        }
        
        // Ensure we stay within available fans
        let available_fans = profile.fan_settings.curves.len().min(fan_count);
        if available_fans == 0 {
            ui.label("No fan curves available");
            return;
        }

        let selected_fan = (*selected_fan_curve).min(available_fans.saturating_sub(1));
        *selected_fan_curve = selected_fan;

        ui.horizontal(|ui| {
            for idx in 0..available_fans {
                let label = fan_info
                    .get(idx)
                    .map(|fan| fan.name.clone())
                    .unwrap_or_else(|| format!("Fan {}", idx));
                if ui.selectable_label(selected_fan == idx, label).clicked() {
                    *selected_fan_curve = idx;
                }
            }
        });

        ui.separator();
        ui.add_space(8.0);

        if let Some(curve) = profile.fan_settings.curves.get_mut(selected_fan) {
            let mut editor = FanCurveEditor::new(curve.fan_id, curve.clone());
            editor.show(ui);
            *curve = editor.get_curve();
        }
    }
}

fn create_default_profile_for_reset(is_standard: bool) -> Profile {
    use lapsphere_common::types::*;
    
    if is_standard {
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
                intel_pstate_status: Some("active".to_string()),
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
                advanced_control: false,
                advanced: GpuAdvancedSettings::default(),
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
    } else {
        Profile::default()
    }
}
