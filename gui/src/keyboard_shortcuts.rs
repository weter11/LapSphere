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
            .with_inner_size([550.0, 550.0])
            .with_min_inner_size([320.0, 260.0]);

        ctx.show_viewport_immediate(viewport_id, viewport_builder, |ctx, class| {
            if !self.show_help {
                return;
            }
            if class == egui::ViewportClass::Embedded {
                egui::Window::new("NVIDIA Overclocking Help")
                    .open(&mut self.show_help)
                    .default_width(550.0)
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            draw_help_contents(ui);
                        });
                    });
                return;
            }

            if ctx.input(|input| input.viewport().close_requested()) {
                self.show_help = false;
                return;
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    draw_help_contents(ui);
                });
            });
        });
    }
}

fn draw_help_contents(ui: &mut egui::Ui) {
    ui.heading("NVIDIA Overclocking Parameter Reference");
    ui.add_space(8.0);

    ui.label("This system provides both static clock locking and dynamic offset adjustment based on telemetry. Below are descriptions for all settings found in the Tuning UI:");
    ui.add_space(12.0);

    ui.heading("Standard & Common Controls");
    ui.add_space(4.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("GPU Locked Clocks (Min/Max)").strong());
        ui.label("Sets a fixed frequency range for the GPU core. Locking both Min and Max to the same value forces a static clock speed.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Memory Locked Clocks (Min/Max)").strong());
        ui.label("Sets a fixed frequency range for the GPU video memory. Similar to core clocks, locking both forces a static frequency.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("GPU Core Offset").strong());
        ui.label("Adds a static offset (MHz) to the entire GPU core frequency curve. Available in Standard control mode.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("GPU Memory Offset").strong());
        ui.label("Adds a static offset (MHz) to the video memory frequency. Available in both Standard and Advanced modes.");
    });
    ui.add_space(12.0);

    ui.heading("Advanced Dynamic Offset Parameters");
    ui.add_space(4.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Dynamic Offset Refresh Rate").strong());
        ui.label("How frequently the daemon recalculates and applies offsets based on live telemetry (seconds).");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Control Flags (Drain/Power/Critical Temp)").strong());
        ui.label("Toggles specific dynamic adjustment logic on or off. Drain control adjusts for voltage drops, Power control for wattage usage, and Critical Temp for safety overrides.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Smart Rounding Threshold").strong());
        ui.label("The step size (MHz) used when applying offsets. Ensures offsets align with hardware steps supported by NVML.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Temperature (°C)").strong());
        ui.label("Defines the range for temperature-based offset scaling. Offsets are calculated relative to these min/max thresholds.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Power limits (W)").strong());
        ui.label("Determines the power usage window. Used to scale the 'Power offset range' based on current GPU power draw.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Frequency thresholds (MHz)").strong());
        ui.label("Defines the clock frequency range used for calculating the 'Base frequency offsets'.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Base frequency offsets (MHz)").strong());
        ui.label("A static offset range that scales with current GPU frequency between the 'Frequency thresholds'. Used to maintain stability in non-boost states.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Low frequency range (MHz)").strong());
        ui.label("Defines the frequency window for 'Low drain offsets' (typically when voltage is below 700mV).");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Low drain offsets (MHz)").strong());
        ui.label("Offset range applied when in the 'Low frequency range'. Increases linearly with temperature.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("High frequency range (MHz)").strong());
        ui.label("Defines the frequency window for 'High drain offsets' (typically when voltage is above 700mV).");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("High drain offsets (MHz)").strong());
        ui.label("Offset range applied when in the 'High frequency range'. Decreases linearly with temperature.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Critical temperature (°C)").strong());
        ui.label("Defines a specific temperature range where 'Critical Temperature Range Control' will override and disable drain offsets to prevent instability.");
    });
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.label(egui::RichText::new("Power offset range (MHz)").strong());
        ui.label("Additional offset added based on current power consumption. Higher at low power, lower at high power.");
    });

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(12.0);

    ui.heading("Implementation Details");
    ui.label("• Offsets are only calculated and applied when the GPU is in P-state 0.");
    ui.label("• Settings are applied to the GPU using the NVIDIA Management Library (NVML).");

    ui.add_space(12.0);
    ui.heading("Smart Rounding");
    ui.label("Offsets are rounded to specific steps (default 15MHz) to ensure compatibility with NVML. A value only rounds up if it is at least 2/3 of the way to the next step.");
    ui.label(egui::RichText::new("Example with threshold=15:").italics());
    ui.label("• 120.0 ➡︎ 120 (8 × 15)");
    ui.label("• 129.9 ➡︎ 120 (not enough to round up)");
    ui.label("• 130.0 ➡︎ 135 (rounds up to 9 × 15)");
}
