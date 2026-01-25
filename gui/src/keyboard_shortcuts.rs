use egui::{Context, Key};
use crate::app::{AppState, Page};

pub struct KeyboardShortcuts {
    show_help: bool,
}

impl KeyboardShortcuts {
    pub fn new() -> Self {
        Self { show_help: false }
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
    
    pub fn handle_shortcuts(&mut self, ctx: &Context, state: &mut AppState) -> bool {
        let mut handled = false;
        
        ctx.input(|i| {
            // Ctrl+1 - Statistics
            if i.modifiers.command && i.key_pressed(Key::Num1) {
                state.current_page = Page::Statistics;
                handled = true;
            }

            // Ctrl+2 - Profiles
            if i.modifiers.command && i.key_pressed(Key::Num2) {
                state.current_page = Page::Profiles;
                handled = true;
            }

            // Ctrl+3 - Tuning
            if i.modifiers.command && i.key_pressed(Key::Num3) {
                state.current_page = Page::Tuning;
                handled = true;
            }

            // Ctrl+4 - Settings
            if i.modifiers.command && i.key_pressed(Key::Num4) {
                state.current_page = Page::Settings;
                handled = true;
            }
            
            // ... etc (rest of shortcuts)
            
            // F1 - Show help
            if i.key_pressed(Key::F1) && !i.modifiers.command {
                log::info!("F1 key pressed, toggling help");
                self.show_help = !self.show_help;
                handled = true;
            }
        });
        
        // Show help window - OUTSIDE of input closure
        if self.show_help {
            self.draw_help_window(ctx);
        }
        
        handled
    }
    
    fn draw_help_window(&mut self, ctx: &Context) {
        let viewport_id = egui::ViewportId::from_hash_of("help_window");
        let viewport_builder = egui::ViewportBuilder::default()
            .with_title("Help")
            .with_inner_size([420.0, 360.0])
            .with_min_inner_size([320.0, 260.0]);

        ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, class| {
            if !self.show_help {
                return;
            }
            if class == egui::ViewportClass::Embedded {
                egui::Window::new("⌨ Keyboard Shortcuts")
                    .open(&mut self.show_help)
                    .default_width(420.0)
                    .show(ctx, |ui| draw_help_contents(ui));
                return;
            }

            if ctx.input(|input| input.viewport().close_requested()) {
                self.show_help = false;
                return;
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                draw_help_contents(ui);
            });
        });
    }
}

fn draw_help_contents(ui: &mut egui::Ui) {
    ui.heading("NVIDIA Overclocking");
    ui.add_space(8.0);
    ui.monospace(
        "DESCRIPTION:\n  This script dynamically adjusts GPU clock offsets based on temperature, power,\n  and frequency using NVIDIA Management Library (NVML). CONFIGURABLE PARAMETERS:\n  \n  Clock Frequency Limits:\n    min_clock              Minimum GPU clock frequency (MHz)\n    max_clock              Maximum GPU clock frequency (MHz)\n  \n  Temperature Thresholds:\n    temperature_min        Minimum temperature for offset calculations (°C)\n    temperature_max        Maximum temperature for offset calculations (°C)\n    critical_temp_min      Critical temperature range minimum (°C)\n    critical_temp_max      Critical temperature range maximum (°C)\n  \n  Power Limits:\n    plimit_min            Minimum power threshold (watts)\n    plimit_max            Maximum power threshold (watts)\n  \n  Frequency Thresholds:\n    frequency_min         Minimum frequency for offset calculations (MHz)\n    frequency_max         Maximum frequency for offset calculations (MHz)\n  \n  Base Frequency Offset:\n    freq_offset_max       Maximum frequency offset at frequency_min (MHz)\n    freq_offset_min       Minimum frequency offset at frequency_max (MHz)\n    \n    → Linearly interpolates between frequency_min and frequency_max\n    → Used for non-P0 states to prevent crashes\n  \n  Low Frequency Range (drain offset):\n    low_freq_min          Low frequency range start (MHz)\n    low_freq_max          Low frequency range end (MHz)\n    drain_offset_lmin     Drain offset at temperature_min (MHz)\n    drain_offset_lmax     Drain offset at temperature_max (MHz)\n    \n    → Applies when GPU frequency is between low_freq_min and low_freq_max\n    → Linearly increases with temperature\n    → GPU voltage should be below 700mV\n  \n  High Frequency Range (drain offset):\n    high_freq_min         High frequency range start (MHz)\n    high_freq_max         High frequency range end (MHz)\n    drain_offset_hmin     Drain offset at temperature_max (MHz)\n    drain_offset_hmax     Drain offset at temperature_min (MHz)\n    \n    → Applies when GPU frequency is between high_freq_min and high_freq_max\n    → Linearly decreases with temperature\n    → GPU voltage should be above 700mV\n  \n  Power-Based Offset:\n    power_offset_max      Maximum power offset at/below plimit_min (MHz)\n    power_offset_min      Minimum power offset at/above plimit_max (MHz)\n    \n    → Linearly interpolates between plimit_min and plimit_max\n  \n  Control Flags:\n    drain_offset_control          Enable/disable drain offset (True/False)\n    power_offset_control          Enable/disable power offset (True/False)\n    critical_temp_range_control   Enable/disable critical temp logic\n                                  (disable drain offset in temps where voltage fluctuates) (True/False)\n  \n  Voltage Monitoring (nvidia-smi 565 or earlier):\n    nvidia_smi_legacy_path        Path to nvidia-smi binary v565 or earlier\n                                  Example: '/opt/nvidia-565/bin/nvidia-smi'\n                                  Leave empty to use system nvidia-smi, if driver version 565 or earlier\n  \n  Refresh Settings:\n    refresh_interval      Update interval in seconds\n\nOFFSET CALCULATION:\n  \n  1. Base Frequency Offset (freq_offset):\n     - Linearly decreases from freq_offset_max to freq_offset_min\n     - Based on current GPU frequency between frequency_min and frequency_max\n  \n  2. Drain Offset (drain_offset):\n     - Applies different offsets for low and high frequency ranges\n     - Low range: Changes with temperature increase\n     - High range: Changes with temperature increase\n     - Critical temp range overrides if enabled to prevent crashes\n  \n  3. Power Offset (power_offset):\n     - Maximum offset at low power consumption\n     - Minimum offset at high power consumption\n     - Linear transition between thresholds\n  \n  4. Total Offset:\n     - P-state 0: Combines all enabled offsets using smart rounding\n     - Other P-states: do not calculate or apply offsets\n     - Applied to GPU using NVML\n\nSMART ROUNDING:\n  - Only increases to next threshold if offset is >= 2/3 of the way\n  - Example with threshold=15:\n    * 120.0 → 120 (8 × 15)\n    * 129.9 → 120 (not enough to round up)\n    * 130.0 → 135 (rounds up to 9 × 15)"
    );
}
