use egui::{Context, Key};
use crate::app::{AppState, Page};

pub struct KeyboardShortcuts {
    show_help: bool,
}

impl KeyboardShortcuts {
    pub fn new() -> Self {
        Self { show_help: false }
    }
    
    pub fn handle_shortcuts(&mut self, ctx: &Context, state: &mut AppState) -> bool {
        let mut handled = false;
        
        ctx.input(|i| {
            // Ctrl+S - Save
            if i.modifiers.command && i.key_pressed(Key::Num1) {
                state.current_page = Page::Statistics;
                handled = true;
            }
            
            // ... etc (rest of shortcuts)
            
            // F1 - Show help
            if i.key_pressed(Key::F1) {
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
        egui::Window::new("⌨️ Keyboard Shortcuts")
            .open(&mut self.show_help)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.heading("Global Shortcuts");
                ui.add_space(8.0);
                
                egui::Grid::new("shortcuts_grid")
                    .num_columns(2)
                    .spacing([40.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Ctrl+S").monospace());
                        ui.label("Save configuration");
                        ui.end_row();
                        
                        ui.label(egui::RichText::new("Ctrl+1").monospace());
                        ui.label("Statistics page");
                        ui.end_row();
                        
                        ui.label(egui::RichText::new("F1").monospace());
                        ui.label("Show this help");
                        ui.end_row();
                    });
            });
    }
}
