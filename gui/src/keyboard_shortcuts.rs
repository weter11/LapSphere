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
            if i.key_pressed(Key::F1) {
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
    ui.heading("Global Shortcuts");
    ui.add_space(8.0);

    egui::Grid::new("shortcuts_grid")
        .num_columns(2)
        .spacing([40.0, 8.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Ctrl+1").monospace());
            ui.label("Statistics page");
            ui.end_row();

            ui.label(egui::RichText::new("Ctrl+2").monospace());
            ui.label("Profiles page");
            ui.end_row();

            ui.label(egui::RichText::new("Ctrl+3").monospace());
            ui.label("Tuning page");
            ui.end_row();

            ui.label(egui::RichText::new("Ctrl+4").monospace());
            ui.label("Settings page");
            ui.end_row();

            ui.label(egui::RichText::new("F1").monospace());
            ui.label("Show help window");
            ui.end_row();
        });

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.heading("Help & NVIDIA Overclocking");
    ui.add_space(8.0);
    ui.label("See Settings → Help for the full NVIDIA overclocking reference.");
}
