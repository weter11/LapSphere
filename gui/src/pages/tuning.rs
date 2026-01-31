use egui::{Ui, ScrollArea, RichText, Slider, ComboBox, TopBottomPanel, Grid};
use crate::app::AppState;
use crate::dbus_client::DbusClient;
use lapsphere_common::types::{KeyboardMode, Profile, FanCurve, KeyboardCapabilities, FanInfo};
use crate::widgets::fan_curve_editor::FanCurveEditor;

pub fn draw(ui: &mut Ui, state: &mut AppState, dbus_client: Option<&DbusClient>, hw_update_tx: tokio::sync::mpsc::UnboundedSender<crate::app::HardwareUpdate>) {
    ui.spacing_mut().slider_width = ui.available_width() * 0.4;
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
                    // Validate fan curves
                    let mut error_msg = None;
                    for curve in &state.config.profiles[idx].fan_settings.curves {
                        let mut temps = std::collections::HashMap::new();
                        for (temp, speed) in &curve.points {
                            if let Some(prev_speed) = temps.insert(*temp, *speed) {
                                if prev_speed != *speed {
                                    error_msg = Some(format!("Error: Fan {} has multiple speeds defined for {}°C. Use the Resort button to check your points.", curve.fan_id, temp));
                                    break;
                                }
                            }
                        }
                        if error_msg.is_some() { break; }
                    }

                    if let Some(msg) = error_msg {
                        state.show_message(msg, true);
                    } else {
                        let _ = state.save_profiles();
                        
                        // Also apply to hardware
                        if let Some(client) = dbus_client {
                            let profile_clone = state.config.profiles[idx].clone();
                            let _rx = client.apply_profile(profile_clone.clone());

                            // Apply GPU settings on save
                            let gpu_info = state.gpu_info.clone();
                            apply_gpu_settings_on_save(client, &profile_clone.gpu_settings, &gpu_info);
                        }
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
            draw_keyboard_tuning(ui, &mut state.config.profiles[idx], keyboard_caps.as_ref(), dbus_client, &mut state.keyboard_brush_color);
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

fn draw_uniwill_tdp_tuning(ui: &mut Ui, profile: &mut Profile, cpu_info: &lapsphere_common::types::CpuInfo) {
    if cpu_info.tdp0.is_some() || cpu_info.tdp1.is_some() || cpu_info.tdp2.is_some() {
        ui.label(RichText::new("Uniwill TDP Control (Watts):").strong());
        ui.add_space(4.0);

        if let Some(current) = cpu_info.tdp0 {
            if let Some((min, max)) = cpu_info.tdp0_range {
                let mut val = profile.cpu_settings.tdp0.unwrap_or(current);
                ui.horizontal(|ui| {
                    ui.label("TDP0 (PL1):");
                    ui.add(Slider::new(&mut val, min..=max).suffix(" W"));
                });
                profile.cpu_settings.tdp0 = Some(val);
            }
        }

        if let Some(current) = cpu_info.tdp1 {
            if let Some((min, max)) = cpu_info.tdp1_range {
                let mut val = profile.cpu_settings.tdp1.unwrap_or(current);
                ui.horizontal(|ui| {
                    ui.label("TDP1 (PL2):");
                    ui.add(Slider::new(&mut val, min..=max).suffix(" W"));
                });
                profile.cpu_settings.tdp1 = Some(val);
            }
        }

        if let Some(current) = cpu_info.tdp2 {
            if let Some((min, max)) = cpu_info.tdp2_range {
                let mut val = profile.cpu_settings.tdp2.unwrap_or(current);
                ui.horizontal(|ui| {
                    ui.label("TDP2 (PL4):");
                    ui.add(Slider::new(&mut val, min..=max).suffix(" W"));
                });
                profile.cpu_settings.tdp2 = Some(val);
            }
        }
        ui.add_space(6.0);
    }
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
    
    // Uniwill TDP section
    draw_uniwill_tdp_tuning(ui, profile, cpu_info);

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
                    .suffix(" MHz")
                    ).changed() {
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
                    .suffix(" MHz")
                    ).changed() {
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
        let nvidia_gpu = gpu_info.iter().find(|g| g.name.contains("NVIDIA"));
        let nvml_index = nvidia_gpu.and_then(|g| g.nvml_index).unwrap_or(0);

        // Fetch ranges if they haven't been fetched yet
        if state.gpu_clock_ranges.is_none() {
            if let Some(client) = dbus_client {
                let client = client.clone();
                let tx = hw_update_tx.clone();
                tokio::spawn(async move {
                    let res = client.get_gpu_clock_ranges(nvml_index).await
                        .map(|r| r.map_err(|e| e.to_string()))
                        .unwrap_or_else(|e| Err(e.to_string()));
                    let _ = tx.send(crate::app::HardwareUpdate::GpuClockRanges(res));
                });
            }
        }
        if state.gpu_core_offset_limits.is_none() {
            if let Some(client) = dbus_client {
                let client = client.clone();
                let tx = hw_update_tx.clone();
                tokio::spawn(async move {
                    let res = client.get_gpu_core_offset_limits(nvml_index).await
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
                    let res = client.get_gpu_memory_offset_limits(nvml_index).await
                        .map(|r| r.map_err(|e| e.to_string()))
                        .unwrap_or_else(|e| Err(e.to_string()));
                    let _ = tx.send(crate::app::HardwareUpdate::GpuMemOffsetLimits(res));
                });
            }
        }

        let fan_info = state.fan_info.clone();
        let profile = &mut state.config.profiles[profile_idx];

        draw_nvidia_fan_tuning(ui, profile, gpu_info, &fan_info);
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        if !tuning_mode_advanced {
            if let Some(gpu) = nvidia_gpu {
                draw_gpu_standard_controls(
                    ui,
                    profile,
                    state.gpu_clock_ranges,
                    state.gpu_mem_clock_ranges,
                    state.gpu_core_offset_limits,
                    state.gpu_mem_offset_limits,
                    gpu,
                    dbus_client,
                    hw_update_tx.clone(),
                    nvml_index,
                );
            }
        }
        if tuning_mode_advanced {
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);

            let mut gpu_oc_poll = state.config.statistics_sections.gpu_overclock_poll_rate;
            draw_gpu_advanced_controls(
                ui,
                profile,
                &mut gpu_oc_poll,
                state.gpu_clock_ranges,
                state.gpu_mem_clock_ranges,
                state.gpu_mem_offset_limits,
            );
            if gpu_oc_poll != state.config.statistics_sections.gpu_overclock_poll_rate {
                state.config.statistics_sections.gpu_overclock_poll_rate = gpu_oc_poll;
                let _ = state.save_settings();
                if let Some(ref handle) = state.coordinator_handle {
                    let _ = handle.update_interval("gpu_overclock".to_string(), std::time::Duration::from_millis(gpu_oc_poll));
                }
                if let Some(client) = dbus_client {
                    let _ = client.update_polling_interval("gpu_overclock", gpu_oc_poll);
                }
            }
        }

        ui.add_space(16.0);
        ui.label(RichText::new("Clock and offset controls are applied when you click Save. They are managed by NVML.").small().italics());
    }
}

fn draw_gpu_standard_controls(
    ui: &mut Ui,
    profile: &mut Profile,
    gpu_clock_ranges: Option<(u32, u32)>,
    _gpu_mem_clock_ranges: Option<(u32, u32)>,
    gpu_core_offset_limits: Option<(i32, i32)>,
    gpu_mem_offset_limits: Option<(i32, i32)>,
    gpu_info: &lapsphere_common::types::GpuInfo,
    dbus_client: Option<&DbusClient>,
    hw_update_tx: tokio::sync::mpsc::UnboundedSender<crate::app::HardwareUpdate>,
    nvml_index: u32,
) {
    let architecture = &gpu_info.architecture;
    ui.add_space(6.0);

    let mut current_core_offset = profile.gpu_settings.core_offset.unwrap_or(0.0);

    // 1. GPU Core Offset
    if !profile.gpu_settings.advanced_control && gpu_info.supports_gpu_offset {
        ui.label(RichText::new("GPU Core Offset:").strong());
        if let Some((min_limit, max_limit)) = gpu_core_offset_limits {
            let step = if let Some(arch) = architecture {
                let arch_l = arch.to_lowercase();
                if arch_l.contains("ada") || arch_l.contains("blackwell") {
                    7.5
                } else {
                    15.0
                }
            } else {
                15.0
            };

            let steps_below = (min_limit.abs() as f32 / step).floor();
            let snapped_min = -steps_below * step;
            let steps_above = (max_limit as f32 / step).floor();
            let snapped_max = steps_above * step;

            if ui.add(Slider::new(&mut current_core_offset, snapped_min..=snapped_max).suffix(" MHz").step_by(step as f64)).changed() {
                profile.gpu_settings.core_offset = Some(current_core_offset);

                // Apply offset in realtime and re-query ranges
                if let Some(client) = dbus_client {
                    let client_c = client.clone();
                    let tx_c = hw_update_tx.clone();
                    let offset = current_core_offset;
                    tokio::spawn(async move {
                        let _ = client_c.set_gpu_core_offset(nvml_index, offset).await;
                        // Re-query ranges immediately
                        let res = client_c.get_gpu_clock_ranges(nvml_index).await
                            .map(|r| r.map_err(|e| e.to_string()))
                            .unwrap_or_else(|e| Err(e.to_string()));
                        let _ = tx_c.send(crate::app::HardwareUpdate::GpuClockRanges(res));
                    });
                }
            }
            profile.gpu_settings.core_offset = Some(current_core_offset);
        }
        ui.add_space(16.0);
    }

    // 2. GPU Locked Clocks (shifted by offset)
    ui.label(RichText::new("GPU Locked Clocks (P-State 0):").strong());
    if let Some((min_range, max_range)) = gpu_clock_ranges {
        let mut min_gpu_clock = profile.gpu_settings.min_gpu_clock.unwrap_or(min_range);
        let mut max_gpu_clock = profile.gpu_settings.max_gpu_clock.unwrap_or(max_range);

        ui.horizontal(|ui| {
            ui.label("Min:");
            if ui.add(Slider::new(&mut min_gpu_clock, min_range..=max_range).suffix(" MHz")).changed() {
                if min_gpu_clock > max_gpu_clock {
                    max_gpu_clock = min_gpu_clock;
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("Max:");
            if ui.add(Slider::new(&mut max_gpu_clock, min_range..=max_range).suffix(" MHz")).changed() {
                if max_gpu_clock < min_gpu_clock {
                    min_gpu_clock = max_gpu_clock;
                }
            }
        });

        profile.gpu_settings.min_gpu_clock = Some(min_gpu_clock);
        profile.gpu_settings.max_gpu_clock = Some(max_gpu_clock);
    } else {
        ui.label("Fetching GPU clock ranges...");
    }

    ui.add_space(16.0);

    // 3. GPU Memory Offset
    if !profile.gpu_settings.advanced_control && gpu_info.supports_mem_offset {
        ui.label(RichText::new("GPU Memory Offset:").strong());
        if let Some((min_limit, max_limit)) = gpu_mem_offset_limits {
            let mut memory_offset = profile.gpu_settings.memory_offset.unwrap_or(0.0);
            ui.add(Slider::new(&mut memory_offset, (min_limit as f32)..=(max_limit as f32)).suffix(" MHz"));
            profile.gpu_settings.memory_offset = Some(memory_offset);
        } else {
            ui.label("Fetching GPU memory offset limits...");
        }
        ui.add_space(16.0);
    } else if !profile.gpu_settings.advanced_control {
        profile.gpu_settings.memory_offset = Some(0.0);
    }

    // GPU Power Limit
    if gpu_info.supports_power_limit {
        ui.label(RichText::new("GPU Power Limit:").strong());
        if let Some((min_w, max_w)) = gpu_info.power_limit_range {
            let mut power_limit = profile.gpu_settings.power_limit.unwrap_or(max_w);
            ui.add(Slider::new(&mut power_limit, min_w..=max_w).suffix(" W"));
            profile.gpu_settings.power_limit = Some(power_limit);
        } else {
            ui.label("Could not determine power limit range.");
        }
        ui.add_space(16.0);
    }
}

fn draw_gpu_advanced_controls(
    ui: &mut Ui,
    profile: &mut Profile,
    gpu_overclock_poll_rate: &mut u64,
    gpu_clock_ranges: Option<(u32, u32)>,
    _gpu_mem_clock_ranges: Option<(u32, u32)>,
    gpu_mem_offset_limits: Option<(i32, i32)>,
) {
    ui.add_space(6.0);
    ui.label(RichText::new("GPU Locked Clocks (Advanced):").strong());
        if let Some((min_range, max_range)) = gpu_clock_ranges {
            let mut min_gpu_clock = profile.gpu_settings.advanced_min_gpu_clock.unwrap_or(min_range);
            let mut max_gpu_clock = profile.gpu_settings.advanced_max_gpu_clock.unwrap_or(max_range);
            ui.horizontal(|ui| {
                ui.label("Min:");
                if ui.add(Slider::new(&mut min_gpu_clock, min_range..=max_range).suffix(" MHz")).changed() {
                    if min_gpu_clock > max_gpu_clock {
                        max_gpu_clock = min_gpu_clock;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Max:");
                if ui.add(Slider::new(&mut max_gpu_clock, min_range..=max_range).suffix(" MHz")).changed() {
                    if max_gpu_clock < min_gpu_clock {
                        min_gpu_clock = max_gpu_clock;
                    }
                }
            });
            profile.gpu_settings.advanced_min_gpu_clock = Some(min_gpu_clock);
            profile.gpu_settings.advanced_max_gpu_clock = Some(max_gpu_clock);
        } else {
            ui.label("Fetching GPU clock ranges...");
    }

    ui.add_space(8.0);
    ui.label(RichText::new("GPU Memory Offset (Advanced):").strong());
    if let Some((min_limit, max_limit)) = gpu_mem_offset_limits {
        let mut advanced_mem_offset = profile.gpu_settings.advanced_memory_offset.unwrap_or(0);
        ui.add(Slider::new(&mut advanced_mem_offset, min_limit..=max_limit).suffix(" MHz"));
        profile.gpu_settings.advanced_memory_offset = Some(advanced_mem_offset);
    } else {
        ui.label("Fetching GPU memory offset limits...");
    }

    ui.label(RichText::new("Advanced GPU Tuning (Dynamic Offsets):").strong());
    ui.add_space(6.0);

    let mut gpu_oc_poll = (*gpu_overclock_poll_rate as f32) / 1000.0;
    ui.horizontal(|ui| {
        ui.label("Dynamic Offset Refresh Rate:");
        if ui.add(Slider::new(&mut gpu_oc_poll, 0.1..=5.0).step_by(0.1).suffix(" s")).changed() {
            *gpu_overclock_poll_rate = (gpu_oc_poll * 1000.0) as u64;
        }
    });
    ui.add_space(8.0);

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

fn draw_nvidia_fan_tuning(
    ui: &mut Ui,
    profile: &mut Profile,
    gpu_info: &[lapsphere_common::types::GpuInfo],
    fan_info: &[FanInfo],
) {
    let old_width = ui.spacing().slider_width;
    ui.spacing_mut().slider_width = ui.available_width() * 0.33;
    for gpu in gpu_info {
        if let Some(nvml_index) = gpu.nvml_index {
            if gpu.name.contains("NVIDIA") {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("🎮 {} Fan Control", gpu.name)).strong());

                let gpu_fans: Vec<&FanInfo> = fan_info.iter()
                    .filter(|f| f.id >= 100 + nvml_index * 10 && f.id < 100 + (nvml_index + 1) * 10)
                    .collect();

                if gpu_fans.is_empty() {
                    ui.label("No controllable fans detected for this GPU via NVML.");
                    continue;
                }

                for fan in gpu_fans {
                    let fan_id_local = fan.id % 10;

                    let mut found = false;
                    for s in profile.gpu_settings.nvidia_fans.iter_mut() {
                        if s.device_index == nvml_index && s.fan_id == fan_id_local {
                            found = true;
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!("Fan {}:", fan_id_local));
                                    ui.checkbox(&mut s.manual, "Manual");

                                    if s.manual {
                                        ui.add(Slider::new(&mut s.speed, 0..=100).suffix("%"));
                                    } else {
                                        ui.label(format!("Auto (Current: {}%)", fan.rpm_or_percent));
                                    }
                                });
                            });
                            break;
                        }
                    }

                    if !found {
                        let new_setting = lapsphere_common::types::NvidiaFanSettings {
                            device_index: nvml_index,
                            fan_id: fan_id_local,
                            speed: fan.rpm_or_percent,
                            manual: false,
                        };
                        profile.gpu_settings.nvidia_fans.push(new_setting);
                    }
                }
            }
        }
    }
    ui.spacing_mut().slider_width = old_width;
}

/// Apply GPU settings when the save button is clicked
fn apply_gpu_settings_on_save(client: &DbusClient, gpu_settings: &lapsphere_common::types::GpuSettings, gpu_info: &[lapsphere_common::types::GpuInfo]) {
    let nvidia_gpu = gpu_info.iter().find(|g| g.name.contains("NVIDIA"));
    let nvidia_gpu_idx = nvidia_gpu.and_then(|g| g.nvml_index).unwrap_or(0);

    if gpu_settings.manual_clocks {
        // Apply GPU locked clocks if set
        if gpu_settings.advanced_control {
            if let (Some(min_clock), Some(max_clock)) =
                (gpu_settings.advanced_min_gpu_clock, gpu_settings.advanced_max_gpu_clock)
            {
                let _ = client.set_gpu_locked_clocks(nvidia_gpu_idx, min_clock, max_clock);
            }
        } else if let (Some(min_clock), Some(max_clock)) = (gpu_settings.min_gpu_clock, gpu_settings.max_gpu_clock) {
            let _ = client.set_gpu_locked_clocks(nvidia_gpu_idx, min_clock, max_clock);
        }
        
        // Apply core offset if set and not in advanced mode (advanced mode is handled by daemon loop)
        if !gpu_settings.advanced_control {
            if let Some(core_offset) = gpu_settings.core_offset {
                let _ = client.set_gpu_core_offset(nvidia_gpu_idx, core_offset);
            }
        }
        
        // Apply memory offset if set
        if gpu_settings.advanced_control {
            if let Some(advanced_offset) = gpu_settings.advanced_memory_offset {
                let _ = client.set_gpu_memory_offset(nvidia_gpu_idx, advanced_offset as f32);
            }
        } else if let Some(memory_offset) = gpu_settings.memory_offset {
            let _ = client.set_gpu_memory_offset(nvidia_gpu_idx, memory_offset);
        }

        // Apply power limit if set
        if let Some(limit) = gpu_settings.power_limit {
            let _ = client.set_gpu_power_limit(nvidia_gpu_idx, limit);
        }
    } else {
        // Reset to factory settings when manual control is disabled
        let _ = client.reset_gpu_clocks(nvidia_gpu_idx);
        // Reset offsets to 0
        let _ = client.set_gpu_core_offset(nvidia_gpu_idx, 0.0);
        let _ = client.set_gpu_memory_offset(nvidia_gpu_idx, 0.0);
    }

    // Apply NVIDIA fan settings
    for fan_setting in &gpu_settings.nvidia_fans {
        if fan_setting.manual {
            let _ = client.set_gpu_fan_speed(fan_setting.device_index, fan_setting.fan_id, fan_setting.speed);
        } else {
            let _ = client.set_gpu_fan_auto(fan_setting.device_index, fan_setting.fan_id);
        }
    }
}

fn draw_keyboard_tuning(
    ui: &mut Ui,
    profile: &mut Profile,
    caps: Option<&KeyboardCapabilities>,
    dbus_client: Option<&DbusClient>,
    brush_color: &mut [u8; 3],
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
                KeyboardMode::PerKeyRGB { .. } => "Per-Key RGB",
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
                    if caps.keyboard_type == lapsphere_common::types::KeyboardType::PerKeyRGB {
                        if ui.selectable_label(current_mode_name == "Per-Key RGB", "Per-Key RGB").clicked() {
                            let mut keys = Vec::new();
                            for _ in 0..caps.num_zones {
                                keys.push(lapsphere_common::types::ZoneColor { r: 255, g: 255, b: 255 });
                            }
                            profile.keyboard_settings.mode = KeyboardMode::PerKeyRGB { keys, brightness: 50 };
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
            KeyboardMode::PerKeyRGB { keys, brightness } => {
                ui.label("Per-Key Color Grid:");
                ui.label(RichText::new("Click a key to set its color to the global picker value below.").small().italics());

                ui.horizontal(|ui| {
                    ui.label("Set All / Brush Color:");
                    ui.color_edit_button_srgb(brush_color);
                    if ui.button("Apply to All").clicked() {
                        for key in keys.iter_mut() {
                            key.r = brush_color[0];
                            key.g = brush_color[1];
                            key.b = brush_color[2];
                        }
                    }
                });

                ui.add_space(4.0);

                ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                        for key in keys.iter_mut() {
                            let color = egui::Color32::from_rgb(key.r, key.g, key.b);
                            let resp = ui.add(egui::Button::new(" ").fill(color).min_size(egui::vec2(20.0, 20.0)));
                            if resp.clicked() {
                                key.r = brush_color[0];
                                key.g = brush_color[1];
                                key.b = brush_color[2];
                            }
                        }
                    });
                });

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
    let old_width = ui.spacing().slider_width;
    ui.spacing_mut().slider_width = ui.available_width() * 0.32;
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
    ui.spacing_mut().slider_width = old_width;
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
                tdp0: None,
                tdp1: None,
                tdp2: None,
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
                core_offset: Some(0.0),
                memory_offset: Some(0.0),
                power_limit: None,
                prime_profile: Some("on-demand".to_string()),
                advanced_control: false,
                advanced: GpuAdvancedSettings::default(),
                advanced_min_gpu_clock: None,
                advanced_max_gpu_clock: None,
                advanced_min_mem_clock: None,
                advanced_max_mem_clock: None,
                advanced_memory_offset: Some(0),
                nvidia_fans: vec![],
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
