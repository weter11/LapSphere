use egui::{Context, Style, Visuals, Color32, Rounding, Stroke, FontId, FontFamily, TextStyle};
use lapsphere_common::types::Theme;

pub struct LapSphereTheme {
    pub visuals: Visuals,
}

impl LapSphereTheme {
    pub fn new(theme: &Theme, system_is_dark: bool) -> Self {
        let visuals = match theme {
            Theme::Auto => if system_is_dark { Self::dark_theme() } else { Self::light_theme() },
            Theme::Dark => Self::dark_theme(),
            Theme::Light => Self::light_theme(),
        };
        
        Self { visuals }
    }
    
    pub fn apply(&self, ctx: &Context) {
        let mut style = Style::default();
        style.visuals = self.visuals.clone();
        
        // Spacing
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.indent = 20.0;
        style.spacing.window_margin = egui::Margin::same(12.0);
        style.spacing.menu_margin = egui::Margin::same(8.0);
        
        // Interaction
        style.interaction.resize_grab_radius_side = 6.0;
        style.interaction.resize_grab_radius_corner = 8.0;
        
        ctx.set_style(style);
    }

        pub fn apply_with_font_size(&self, ctx: &Context, font_size: &lapsphere_common::types::FontSize) {
        use lapsphere_common::types::FontSize;
        
        let mut style = (*ctx.style()).clone();
        style.visuals = self.visuals.clone();
        
        // Text styles with font size
        let (heading, body, button, small, mono) = match font_size {
            FontSize::Small => (18.0, 12.0, 12.0, 9.0, 11.0),
            FontSize::Medium => (22.0, 14.0, 14.0, 11.0, 13.0),
            FontSize::Large => (26.0, 16.0, 16.0, 13.0, 15.0),
        };
        
        let mut text_styles = std::collections::BTreeMap::new();
        text_styles.insert(
            TextStyle::Heading,
            FontId::new(heading, FontFamily::Proportional),
        );
        text_styles.insert(
            TextStyle::Body,
            FontId::new(body, FontFamily::Proportional),
        );
        text_styles.insert(
            TextStyle::Monospace,
            FontId::new(mono, FontFamily::Monospace),
        );
        text_styles.insert(
            TextStyle::Button,
            FontId::new(button, FontFamily::Proportional),
        );
        text_styles.insert(
            TextStyle::Small,
            FontId::new(small, FontFamily::Proportional),
        );
        
        style.text_styles = text_styles;
        ctx.set_style(style);
    }
    
    fn dark_theme() -> Visuals {
        Visuals {
            dark_mode: true,
            
            // Colors
            widgets: egui::style::Widgets {
                noninteractive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(32, 33, 36),
                    weak_bg_fill: Color32::from_rgb(32, 33, 36),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(60, 63, 68)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_rgb(220, 220, 220)),
                    expansion: 0.0,
                },
                inactive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(45, 47, 52),
                    weak_bg_fill: Color32::from_rgb(45, 47, 52),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(70, 73, 78)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_rgb(200, 200, 200)),
                    expansion: 0.0,
                },
                hovered: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(55, 58, 64),
                    weak_bg_fill: Color32::from_rgb(55, 58, 64),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(90, 93, 98)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.5, Color32::from_rgb(230, 230, 230)),
                    expansion: 1.0,
                },
                active: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(65, 120, 200),
                    weak_bg_fill: Color32::from_rgb(65, 120, 200),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(85, 140, 220)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(2.0, Color32::WHITE),
                    expansion: 1.0,
                },
                open: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(50, 52, 58),
                    weak_bg_fill: Color32::from_rgb(50, 52, 58),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(80, 83, 88)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_rgb(220, 220, 220)),
                    expansion: 0.0,
                },
            },
            
            // Selection color (for sliders, checkboxes)
            selection: egui::style::Selection {
                bg_fill: Color32::from_rgb(65, 120, 200),
                stroke: Stroke::new(1.0, Color32::from_rgb(85, 140, 220)),
            },
            
            // Hyperlinks
            hyperlink_color: Color32::from_rgb(90, 170, 255),
            
            // Window
            window_fill: Color32::from_rgb(25, 26, 29),
            window_stroke: Stroke::new(1.0, Color32::from_rgb(50, 52, 56)),
            window_shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 8.0),
                blur: 16.0,
                spread: 0.0,
                color: Color32::from_black_alpha(100),
            },
            window_rounding: Rounding::same(8.0),
            
            // Panel
            panel_fill: Color32::from_rgb(28, 29, 32),
            
            // Popup
            popup_shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 12.0,
                spread: 0.0,
                color: Color32::from_black_alpha(120),
            },
            
            // Text colors
            override_text_color: Some(Color32::from_rgb(220, 220, 220)),
            warn_fg_color: Color32::from_rgb(255, 165, 0),
            error_fg_color: Color32::from_rgb(255, 80, 80),
            
            // Other
            faint_bg_color: Color32::from_rgb(40, 42, 46),
            extreme_bg_color: Color32::from_rgb(15, 16, 18),
            code_bg_color: Color32::from_rgb(35, 37, 40),
            
            ..Visuals::dark()
        }
    }
    
    fn light_theme() -> Visuals {
        Visuals {
            dark_mode: false,

            widgets: egui::style::Widgets {
                noninteractive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(252),
                    weak_bg_fill: Color32::from_gray(252),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(210)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_gray(20)),
                    expansion: 0.0,
                },
                inactive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(236),
                    weak_bg_fill: Color32::from_gray(236),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(195)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_gray(30)),
                    expansion: 0.0,
                },
                hovered: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(226),
                    weak_bg_fill: Color32::from_gray(226),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(175)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.5, Color32::BLACK),
                    expansion: 1.0,
                },
                active: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(90, 150, 220),
                    weak_bg_fill: Color32::from_rgb(90, 150, 220),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(60, 120, 190)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(2.0, Color32::from_gray(20)),
                    expansion: 1.0,
                },
                open: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(238),
                    weak_bg_fill: Color32::from_gray(238),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(190)),
                    rounding: Rounding::same(6.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_gray(20)),
                    expansion: 0.0,
                },
            },

            selection: egui::style::Selection {
                bg_fill: Color32::from_rgb(90, 150, 220),
                stroke: Stroke::new(1.0, Color32::from_rgb(60, 120, 190)),
            },

            hyperlink_color: Color32::from_rgb(40, 100, 200),

            window_fill: Color32::from_gray(245),
            window_stroke: Stroke::new(1.0, Color32::from_gray(210)),
            window_shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 8.0),
                blur: 16.0,
                spread: 0.0,
                color: Color32::from_black_alpha(30),
            },
            window_rounding: Rounding::same(8.0),

            panel_fill: Color32::from_gray(252),

            override_text_color: Some(Color32::from_gray(20)),
            warn_fg_color: Color32::from_rgb(180, 100, 0),
            error_fg_color: Color32::from_rgb(180, 20, 20),

            faint_bg_color: Color32::from_gray(236),
            extreme_bg_color: Color32::from_gray(220),
            code_bg_color: Color32::from_gray(230),

            ..Visuals::light()
        }
    }
}

// Helper functions for consistent colors
pub fn temp_color(temp: f32) -> Color32 {
    if temp < 50.0 {
        Color32::from_rgb(80, 180, 240)  // Cool blue
    } else if temp < 70.0 {
        Color32::from_rgb(100, 200, 120) // Green
    } else if temp < 85.0 {
        Color32::from_rgb(255, 200, 60)  // Yellow/orange
    } else {
        Color32::from_rgb(255, 80, 80)   // Hot red
    }
}

pub fn load_color(load: f32) -> Color32 {
    if load < 30.0 {
        Color32::from_rgb(80, 180, 240)  // Low - blue
    } else if load < 60.0 {
        Color32::from_rgb(100, 200, 120) // Medium - green
    } else if load < 85.0 {
        Color32::from_rgb(255, 200, 60)  // High - yellow
    } else {
        Color32::from_rgb(255, 100, 60)  // Very high - orange/red
    }
}

pub fn power_color(watts: f32) -> Color32 {
    if watts < 10.0 {
        Color32::from_rgb(100, 200, 120) // Low power - green
    } else if watts < 25.0 {
        Color32::from_rgb(100, 180, 240) // Medium - blue
    } else if watts < 45.0 {
        Color32::from_rgb(255, 200, 60)  // High - yellow
    } else {
        Color32::from_rgb(255, 100, 60)  // Very high - orange
    }
}
