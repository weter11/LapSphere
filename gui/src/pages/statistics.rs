use egui::{Ui, ScrollArea, CollapsingHeader, Grid, ProgressBar, RichText};
use egui::Color32;
use crate::app::AppState;
use webbrowser;
use crate::theme::{temp_color, load_color, power_color};

pub const STATISTICS_SECTIONS: [(&str, &str); 8] = [
    ("SystemInfo", "System Info"),
    ("CPU", "CPU"),
    ("Memory", "Memory"),
    ("GPU", "GPU"),
    ("Battery", "Battery"),
    ("WiFi", "WiFi"),
    ("Storage", "Storage"),
    ("Fans", "Fans"),
];

const WIFI_NOT_CONNECTED: &str = "Not connected";

pub fn normalize_section_order(order: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for section in order {
        if STATISTICS_SECTIONS.iter().any(|(key, _)| key == section)
            && !normalized.iter().any(|item| item == section)
        {
            normalized.push(section.clone());
        }
    }
    for (key, _) in STATISTICS_SECTIONS {
        if !normalized.iter().any(|item| item == key) {
            normalized.push(key.to_string());
        }
    }
    normalized
}

pub fn draw(ui: &mut Ui, state: &mut AppState) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.heading("📊 Statistics");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("📂 Crash Reports").on_hover_text("Open folder with crash reports").clicked() {
                let crash_dir = crate::app::get_crash_dir();
                let _ = std::fs::create_dir_all(&crash_dir);
                let _ = webbrowser::open(&crash_dir);
            }
        });
    });
    ui.add_space(6.0);
    ui.separator();

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);

            let order = normalize_section_order(&state.config.statistics_sections.section_order);
            if order != state.config.statistics_sections.section_order {
                state.config.statistics_sections.section_order = order.clone();
            }

            for section in order {
                match section.as_str() {
                    "SystemInfo" if state.config.statistics_sections.show_system_info => {
                        draw_system_info(ui, state);
                        ui.add_space(6.0);
                    }
                    "CPU" if state.config.statistics_sections.show_cpu => {
                        draw_cpu_info(ui, state);
                        ui.add_space(6.0);
                    }
                    "Memory" if state.config.statistics_sections.show_memory => {
                        draw_memory_info(ui, state);
                        ui.add_space(6.0);
                    }
                    "GPU" if state.config.statistics_sections.show_gpu => {
                        draw_gpu_info(ui, state);
                        ui.add_space(6.0);
                    }
                    "Battery" if state.config.statistics_sections.show_battery => {
                        draw_battery_info(ui, state);
                        ui.add_space(6.0);
                    }
                    "WiFi" if state.config.statistics_sections.show_wifi => {
                        draw_wifi_info(ui, state);
                        ui.add_space(6.0);
                    }
                    "Storage" if state.config.statistics_sections.show_storage => {
                        draw_storage_info(ui, state);
                        ui.add_space(6.0);
                    }
                    "Fans" if state.config.statistics_sections.show_fans => {
                        draw_fan_info(ui, state);
                        ui.add_space(6.0);
                    }
                    _ => {}
                }
            }
        });
}

fn draw_memory_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("🐏 Memory (RAM)").heading())
        .default_open(true)
        .show(ui, |ui| {
            if let Some(ref mem) = state.memory_info {
                Grid::new("memory_grid")
                    .num_columns(2)
                    .spacing([36.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Usage:");
                        ui.add(
                            ProgressBar::new(mem.used_percent / 100.0)
                                .text(format!("{:.1}%", mem.used_percent))
                                .fill(load_color(mem.used_percent))
                        );
                        ui.end_row();

                        ui.label("Used:");
                        ui.label(format!("{:.2} GiB", mem.used_gib));
                        ui.end_row();

                        ui.label("Available:");
                        ui.label(format!("{:.2} GiB", mem.available_gib));
                        ui.end_row();

                        ui.label("Total:");
                        ui.label(format!("{:.2} GiB", mem.total_gib));
                        ui.end_row();
                    });
            } else {
                ui.spinner();
                ui.label("Loading memory information...");
            }
        });
}

fn draw_system_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("📊 System Information").heading())
        .default_open(true)  // Changed to true
        .show(ui, |ui| {
            if let Some(ref info) = state.system_info {
                Grid::new("system_grid")
                    .num_columns(2)
                    .spacing([36.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Notebook Model:");
                        ui.label(&info.product_name);
                        ui.end_row();
                        
                        ui.label("Manufacturer:");
                        ui.label(&info.manufacturer);
                        ui.end_row();
                        
                        ui.label("BIOS Version:");
                        ui.label(&info.bios_version);
                        ui.end_row();
                    });
            } else {
                ui.spinner();
                ui.label("Loading system information...");
            }
        });
}

fn draw_cpu_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("🖥 CPU").heading())
        .default_open(true)
        .show(ui, |ui| {
            if let Some(ref cpu) = state.cpu_info {
                Grid::new("cpu_grid")
                    .num_columns(2)
                    .spacing([36.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Processor:");
                        ui.label(&cpu.name);
                        ui.end_row();
                        
                        ui.label("Median Frequency:");
                        ui.label(RichText::new(format!("{} MHz", cpu.median_frequency / 1000))
                            .monospace());
                        ui.end_row();
                        
                        ui.label("Median Load:");
                        ui.horizontal(|ui| {
                            ui.add(
                                ProgressBar::new(cpu.median_load / 100.0)
                                    .text(format!("{:.1}%", cpu.median_load))
                                    .fill(load_color(cpu.median_load))
                            );
                        });
                        ui.end_row();
                        
                        ui.label("Package Temperature:");
                        ui.colored_label(
                            temp_color(cpu.package_temp),
                            RichText::new(format!("{:.1}°C", cpu.package_temp))
                                .strong()
                                .monospace()
                        );
                        ui.end_row();
                        
                        if !cpu.all_power_sources.is_empty() && cpu.all_power_sources.len() > 1 {
                            ui.label("All Power Sources:");
                            ui.vertical(|ui| {
                                for source in &cpu.all_power_sources {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&source.name).small());
                                        ui.label(RichText::new(format!("{:.1} W", source.value))
                                            .small()
                                            .monospace());
                                    });
                                }
                            });
                            ui.end_row();
                        }
                        
                        ui.label("");
                        ui.separator();
                        ui.end_row();
                        
                        if cpu.capabilities.has_scaling_driver {
                            ui.label("Scaling Driver:");
                            ui.label(&cpu.scaling_driver);
                            ui.end_row();
                        }
                        
                        if cpu.capabilities.has_scaling_governor {
                            ui.label("Governor:");
                            ui.label(RichText::new(&cpu.governor).monospace());
                            ui.end_row();
                        }
                        
                        if cpu.capabilities.has_energy_performance_preference {
                            if let Some(ref epp) = cpu.energy_performance_preference {
                                ui.label("EPP:");
                                ui.label(epp);
                                ui.end_row();
                            }
                        }
                        
                        if cpu.capabilities.has_boost {
                            ui.label("CPU Boost:");
                            ui.label(if cpu.boost_enabled { "✅ Enabled" } else { "❌ Disabled" });
                            ui.end_row();
                        }
                        
                        if cpu.capabilities.has_smt {
                            ui.label("SMT / Hyperthreading:");
                            ui.label(if cpu.smt_enabled { "✅ Enabled" } else { "❌ Disabled" });
                            ui.end_row();
                        }
                        
                        if cpu.capabilities.has_amd_pstate {
                            if let Some(ref status) = cpu.amd_pstate_status {
                                ui.label("AMD P-State:");
                                ui.label(format!("{} mode", status));
                                ui.end_row();
                            }
                        }
                    });
                
                // Per-core details (still collapsed by default)
                ui.add_space(6.0);
                CollapsingHeader::new(format!("Core Details ({} cores)", cpu.cores.len()))
                    .default_open(false)
                    .show(ui, |ui| {
                        Grid::new("cores_grid")
                            .num_columns(4)
                            .spacing([20.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label(RichText::new("Core").strong());
                                ui.label(RichText::new("Frequency").strong());
                                ui.label(RichText::new("Load").strong());
                                ui.label(RichText::new("Temp").strong());
                                ui.end_row();
                                
                                for core in &cpu.cores {
                                    ui.label(format!("CPU {}", core.id));
                                    ui.label(RichText::new(format!("{} MHz", core.frequency / 1000))
                                        .monospace());
                                    ui.add(
                                        ProgressBar::new(core.load / 100.0)
                                            .text(format!("{:.0}%", core.load))
                                            .desired_width(80.0)
                                    );
                                    ui.colored_label(
                                        temp_color(core.temperature),
                                        format!("{:.0}°C", core.temperature)
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            } else {
                ui.spinner();
                ui.label("Loading CPU information...");
            }
        });
}

fn draw_gpu_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("🎮 GPU").heading())
        .default_open(true)  // Changed to true
        .show(ui, |ui| {
            if !state.gpu_info.is_empty() {
                for (idx, gpu) in state.gpu_info.iter().enumerate() {
                    if idx > 0 {
                        ui.separator();
                        ui.add_space(6.0);
                    }
                    
                    ui.label(RichText::new(&gpu.name).strong());
                    Grid::new(format!("gpu_grid_{}", idx))
                        .num_columns(2)
                        .spacing([36.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Type:");
                            ui.label(if gpu.gpu_type == lapsphere_common::types::GpuType::Integrated {
                                "Integrated"
                            } else {
                                "Discrete"
                            });
                            ui.end_row();
                            
                            ui.label("Status:");
                            ui.label(&gpu.status);
                            ui.end_row();
                            
                            if let Some(freq) = gpu.frequency {
                                ui.label("Core Frequency:");
                                ui.horizontal(|ui| {
                                    ui.label(format!("{} MHz", freq));
                                    if let Some((min, max)) = gpu.core_clock_range {
                                        ui.label(RichText::new(format!(" (Range: {} - {} MHz)", min, max)).small().italics());
                                    }
                                });
                                ui.end_row();
                            }

                            if let (Some(min), Some(max)) = (gpu.min_core_clock, gpu.max_core_clock) {
                                ui.label("Locked Core Clocks:");
                                ui.label(RichText::new(format!("{} - {} MHz", min, max)).strong());
                                ui.end_row();
                            }

                            if let Some(mem_freq) = gpu.memory_frequency {
                                ui.label("Memory Frequency:");
                                ui.horizontal(|ui| {
                                    ui.label(format!("{} MHz", mem_freq));
                                    if let Some((min, max)) = gpu.memory_clock_range {
                                        ui.label(RichText::new(format!(" (Range: {} - {} MHz)", min, max)).small().italics());
                                    }
                                });
                                ui.end_row();
                            }

                            if let (Some(min), Some(max)) = (gpu.min_memory_clock, gpu.max_memory_clock) {
                                ui.label("Locked Memory Clocks:");
                                ui.label(RichText::new(format!("{} - {} MHz", min, max)).strong());
                                ui.end_row();
                            }
                            
                            if let Some(temp) = gpu.temperature {
                                ui.label("Temperature:");
                                ui.colored_label(
                                    temp_color(temp),
                                    format!("{:.1}°C", temp)
                                );
                                ui.end_row();
                            }
                            
                            if let Some(hotspot_temp) = gpu.hotspot_temperature {
                                ui.label("Hotspot Temperature:");
                                ui.colored_label(
                                    temp_color(hotspot_temp),
                                    format!("{:.1}°C", hotspot_temp)
                                );
                                ui.end_row();
                            }

                            if let Some(mem_temp) = gpu.memory_temperature {
                                ui.label("Memory Temperature:");
                                ui.colored_label(
                                    temp_color(mem_temp),
                                    format!("{:.1}°C", mem_temp)
                                );
                                ui.end_row();
                            }
                            
                            if let Some(load) = gpu.load {
                                ui.label("Load:");
                                ui.add(ProgressBar::new(load / 100.0)
                                    .text(format!("{:.1}%", load)));
                                ui.end_row();
                            }
                            
                            if let Some(power) = gpu.power {
                                ui.label("Power:");
                                ui.colored_label(
                                    power_color(power),
                                    format!("{:.1} W", power)
                                );
                                ui.end_row();
                            }

                            if let Some(voltage) = gpu.voltage {
                                ui.label("Voltage:");
                                ui.label(format!("{:.3} V", voltage));
                                ui.end_row();
                            }

                            if let Some(ref v_type) = gpu.vram_type {
                                ui.label("VRAM Type:");
                                ui.label(v_type);
                                ui.end_row();
                            }

                            if let Some(ref v_vendor) = gpu.vram_vendor {
                                ui.label("VRAM Vendor:");
                                ui.label(v_vendor);
                                ui.end_row();
                            }

                            if let Some(v_bus) = gpu.vram_bus_width {
                                ui.label("Bus Width:");
                                ui.label(format!("{}-bit", v_bus));
                                ui.end_row();
                            }

                            if let Some(v_bw) = gpu.vram_bandwidth {
                                ui.label("VRAM Bandwidth:");
                                ui.label(format!("{:.1} GB/s", v_bw));
                                ui.end_row();
                            }


                            if let Some(fo) = gpu.freq_offset {
                                ui.label("Freq Offset:");
                                ui.label(format!("{}{} MHz", if fo >= 0 { "+" } else { "" }, fo));
                                ui.end_row();
                            }

                            if let Some(do_) = gpu.drain_offset {
                                ui.label("Drain Offset:");
                                ui.label(format!("{}{} MHz", if do_ >= 0 { "+" } else { "" }, do_));
                                ui.end_row();
                            }

                            if let Some(po) = gpu.power_offset {
                                ui.label("Power Offset:");
                                ui.label(format!("{}{} MHz", if po >= 0 { "+" } else { "" }, po));
                                ui.end_row();
                            }

                            if let Some(to) = gpu.total_offset {
                                ui.label("Total Offset:");
                                ui.label(RichText::new(format!("{}{} MHz", if to >= 0 { "+" } else { "" }, to)).strong());
                                ui.end_row();
                            }
                        });
                }
            } else {
                ui.label("No GPU detected");
            }
        });
}

fn draw_battery_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("🔋 Battery").heading())
        .default_open(true)
        .show(ui, |ui| {
            if let Some(ref battery) = state.battery_info {
                Grid::new("battery_grid")
                    .num_columns(2)
                    .spacing([36.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Capacity:");
                        ui.horizontal(|ui| {
                            ui.add(
                                ProgressBar::new(battery.charge_percent as f32 / 100.0)
                                    .text(format!("{}%", battery.charge_percent))
                                    .desired_width(200.0)
                            );
                        });
                        ui.end_row();
                        
                        ui.label("Voltage:");
                        ui.label(format!("{:.2} V", battery.voltage_mv as f64 / 1000.0));
                        ui.end_row();
                        
                        ui.label("Current:");
                        let current_a = battery.current_ma as f64 / 1000.0;
                        ui.label(format!("{:.2} A", current_a.abs()));
                        ui.end_row();
                        
                        ui.label("Status:");
                        ui.label(&battery.status);
                        ui.end_row();

                        let power_w = (battery.voltage_mv as f64 * battery.current_ma as f64) / 1_000_000.0;
                        if power_w.abs() > 0.1 {
                            ui.label("Power:");
                            ui.colored_label(
                                power_color(power_w.abs() as f32),
                                format!("{:.1} W", power_w.abs())
                            );
                            ui.end_row();
                        }
                        
                        if let Some(start) = battery.charge_start_threshold {
                            ui.label("Charge Start:");
                            ui.label(format!("{}%", start));
                            ui.end_row();
                        }
                        
                        if let Some(end) = battery.charge_end_threshold {
                            ui.label("Charge End:");
                            ui.label(format!("{}%", end));
                            ui.end_row();
                        }
                    });
            } else {
                ui.label("No battery detected");
            }
        });
}

fn draw_wifi_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("📶 WiFi").heading())
        .default_open(true)
        .show(ui, |ui| {
            if !state.wifi_info.is_empty() {
                for wifi in &state.wifi_info {
                    ui.label(RichText::new(format!("Interface: {}", wifi.interface)).strong());
                    
                    Grid::new(format!("wifi_grid_{}", wifi.interface))
                        .num_columns(2)
                        .spacing([40.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            // SSID
                            ui.label("SSID:");
                            if let Some(ref ssid) = wifi.ssid {
                                ui.label(RichText::new(ssid).strong());
                            } else {
                                ui.label(RichText::new(WIFI_NOT_CONNECTED).italics().weak());
                            }
                            ui.end_row();

                            // Signal level
                            ui.label("Signal Level:");
                            if let Some(signal) = wifi.signal_level {
                                ui.horizontal(|ui| {
                                    let signal_percent = ((signal + 90) as f32 / 60.0).clamp(0.0, 1.0);

                                    let color = if signal_percent > 0.7 {
                                        Color32::from_rgb(100, 200, 120)
                                    } else if signal_percent > 0.4 {
                                        Color32::from_rgb(255, 200, 60)
                                    } else {
                                        Color32::from_rgb(255, 100, 80)
                                    };

                                    let progress_bar = ProgressBar::new(signal_percent)
                                        .text(RichText::new(format!("{} dBm", signal)).color(Color32::BLACK))
                                        .fill(color);
                                    ui.add(progress_bar);
                                });
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();

                            // Received Data
                            ui.label("Received Data:");
                            if let Some(rx_bytes) = wifi.rx_bytes {
                                ui.label(RichText::new(format_bytes(rx_bytes)).monospace());
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();

                            // Sent Data
                            ui.label("Sent Data:");
                            if let Some(tx_bytes) = wifi.tx_bytes {
                                ui.label(RichText::new(format_bytes(tx_bytes)).monospace());
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();

                            // Channel Number
                            ui.label("Channel Number:");
                            if let Some(channel) = wifi.channel {
                                ui.label(format!("{}", channel));
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();

                            // Channel Width
                            ui.label("Channel Width:");
                            if let Some(width) = wifi.channel_width {
                                ui.label(format!("{} MHz", width));
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();

                            // PHY Rate
                            ui.label("PHY Rate (RX/TX link bitrates):");
                            if wifi.rx_bitrate.is_some() || wifi.tx_bitrate.is_some() {
                                ui.horizontal(|ui| {
                                    if let Some(rx) = wifi.rx_bitrate {
                                        ui.label(RichText::new(format!("RX: {:.1} Mbps", rx)).small().monospace());
                                    }
                                    if let Some(tx) = wifi.tx_bitrate {
                                        ui.label(RichText::new(format!(" TX: {:.1} Mbps", tx)).small().monospace());
                                    }
                                });
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();

                            // Actual Throughput
                            ui.label("Actual Throughput:");
                            if wifi.rx_rate.is_some() || wifi.tx_rate.is_some() {
                                ui.horizontal(|ui| {
                                    if let Some(rx) = wifi.rx_rate {
                                        ui.label(RichText::new(format!("↓ {:.2} Mbps", rx))
                                            .monospace()
                                            .color(Color32::from_rgb(100, 200, 255)));
                                    }
                                    if let Some(tx) = wifi.tx_rate {
                                        ui.label(RichText::new(format!(" ↑ {:.2} Mbps", tx))
                                            .monospace()
                                            .color(Color32::from_rgb(255, 150, 100)));
                                    }
                                });
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();
                            
                            // Temperature
                            ui.label("Network Adapter Temperature:");
                            if let Some(temp) = wifi.temperature {
                                ui.colored_label(
                                    temp_color(temp),
                                    RichText::new(format!("{:.1}°C", temp)).monospace()
                                );
                            } else {
                                ui.label(RichText::new("—").weak());
                            }
                            ui.end_row();
                        });
                    
                    ui.add_space(6.0);
                }
            } else {
                ui.label("No WiFi interface detected");
            }
        });
}

fn draw_storage_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("💾 Storage").heading())
        .default_open(true)
        .show(ui, |ui| {
            if !state.storage_device_info.is_empty() {
                for device in &state.storage_device_info {
                    ui.label(RichText::new(&device.model).strong());
                    Grid::new(format!("storage_device_grid_{}", device.device))
                        .num_columns(2)
                        .spacing([36.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Device:");
                            ui.label(RichText::new(&device.device).monospace());
                            ui.end_row();

                            ui.label("Size:");
                            ui.label(format!("{:.1} GB", device.size_gb));
                            ui.end_row();

                            if let Some(temp) = device.temperature {
                                ui.label("Temperature:");
                                ui.colored_label(
                                    temp_color(temp),
                                    format!("{:.1}°C", temp)
                                );
                                ui.end_row();
                            }

                            if let Some(read_speed) = device.read_speed {
                                ui.label("Read Speed:");
                                ui.label(RichText::new(format!("{:.1} MB/s", read_speed)).monospace());
                                ui.end_row();
                            }

                            if let Some(write_speed) = device.write_speed {
                                ui.label("Write Speed:");
                                ui.label(RichText::new(format!("{:.1} MB/s", write_speed)).monospace());
                                ui.end_row();
                            }

                            if let Some(read_iops) = device.read_iops {
                                ui.label("Read IOPS:");
                                ui.label(RichText::new(format!("{:.0} IOPS", read_iops)).monospace());
                                ui.end_row();
                            }

                            if let Some(write_iops) = device.write_iops {
                                ui.label("Write IOPS:");
                                ui.label(RichText::new(format!("{:.0} IOPS", write_iops)).monospace());
                                ui.end_row();
                            }
                        });
                    ui.add_space(6.0);
                }
            } else {
                ui.label("No storage devices detected");
            }

            if !state.mount_info.is_empty() {
                ui.separator();
                for mount in &state.mount_info {
                    ui.label(RichText::new(&mount.mount_point).strong());
                    Grid::new(format!("mount_grid_{}", mount.mount_point))
                        .num_columns(2)
                        .spacing([36.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Usage:");
                            ui.horizontal(|ui| {
                                ui.add(
                                    ProgressBar::new(mount.used_percent as f32 / 100.0)
                                        .text(format!("{:.1}%", mount.used_percent))
                                        .desired_width(200.0)
                                );
                            });
                            ui.end_row();

                            ui.label("Free Space:");
                            ui.label(format!("{:.1} GB", mount.total_gb as f64 - mount.used_gb as f64));
                            ui.end_row();

                            ui.label("Filesystem:");
                            ui.label(&mount.filesystem_type);
                            ui.end_row();
                        });
                    ui.add_space(6.0);
                }
            }
        });
}

fn draw_fan_info(ui: &mut Ui, state: &AppState) {
    CollapsingHeader::new(RichText::new("💨 Fans").heading())
        .default_open(true)
        .show(ui, |ui| {
            if !state.fan_info.is_empty() {
                Grid::new("fans_grid")
                    .num_columns(4)
                    .spacing([36.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Fan").strong());
                        ui.label(RichText::new("Speed").strong());
                        ui.label(RichText::new("Temperature").strong());
                        ui.label(RichText::new("Mode").strong());
                        ui.end_row();
                        
                        for fan in &state.fan_info {
                            ui.label(&fan.name);
                            
                            ui.horizontal(|ui| {
                                let speed_pct = if fan.is_rpm {
                                    (fan.rpm_or_percent as f32 / 5000.0).min(1.0)
                                } else {
                                    fan.rpm_or_percent as f32 / 100.0
                                };
                                
                                ui.add(
                                    ProgressBar::new(speed_pct)
                                        .text(if fan.is_rpm {
                                            format!("{} RPM", fan.rpm_or_percent)
                                        } else {
                                            format!("{}%", fan.rpm_or_percent)
                                        })
                                        .desired_width(120.0)
                                );
                            });
                            
                            if let Some(temp) = fan.temperature {
                                ui.colored_label(
                                    temp_color(temp),
                                    format!("{:.1}°C", temp)
                                );
                            } else {
                                ui.label("—");
                            }

                            if let Some(ref mode) = fan.mode {
                                ui.label(mode);
                            } else {
                                ui.label("Auto");
                            }
                            
                            ui.end_row();
                        }
                    });
            } else {
                ui.label("No fan information available");
            }
        });
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.2} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.2} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.2} KiB", bytes_f / KIB)
    } else {
        format!("{} B", bytes)
    }
}
