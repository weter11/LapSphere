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

    pub fn apply_with_font_size(&self, ctx: &Context, font_size: &lapsphere_common::types::FontSize) {
        use lapsphere_common::types::FontSize;
        
        let mut style = (*ctx.style()).clone();
        style.visuals = self.visuals.clone();
        
        // Text styles with font size - adjusted values to keep UI compact
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
        
        // Keep spacing and padding INDEPENDENT of font size for compact UI
        // These values remain constant regardless of font size setting
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);  // Reduced for compactness
        style.spacing.button_padding = egui::vec2(10.0, 4.0);  // Reduced vertical padding
        style.spacing.indent = 16.0;  // Slightly reduced
        style.spacing.window_margin = egui::Margin::same(10.0);  // Reduced margin
        style.spacing.menu_margin = egui::Margin::same(6.0);  // Reduced margin
        
        // Interaction settings remain independent of font size
        style.interaction.resize_grab_radius_side = 6.0;
        style.interaction.resize_grab_radius_corner = 8.0;
        
        ctx.set_style(style);
    }
    
    fn dark_theme() -> Visuals {
        Visuals {
            dark_mode: true,
            
            // Modern dark theme with improved contrast and readability
            widgets: egui::style::Widgets {
                noninteractive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(38, 40, 44),
                    weak_bg_fill: Color32::from_rgb(38, 40, 44),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(55, 58, 64)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_rgb(225, 228, 232)),
                    expansion: 0.0,
                },
                inactive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(48, 51, 57),
                    weak_bg_fill: Color32::from_rgb(48, 51, 57),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(65, 70, 78)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_rgb(205, 210, 220)),
                    expansion: 0.0,
                },
                hovered: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(58, 62, 70),
                    weak_bg_fill: Color32::from_rgb(58, 62, 70),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(85, 92, 102)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.5, Color32::from_rgb(235, 238, 242)),
                    expansion: 1.0,
                },
                active: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(70, 130, 215),
                    weak_bg_fill: Color32::from_rgb(70, 130, 215),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(90, 150, 235)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(2.0, Color32::WHITE),
                    expansion: 1.0,
                },
                open: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(52, 55, 62),
                    weak_bg_fill: Color32::from_rgb(52, 55, 62),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(75, 80, 88)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_rgb(225, 228, 232)),
                    expansion: 0.0,
                },
            },
            
            // Selection color (for sliders, checkboxes)
            selection: egui::style::Selection {
                bg_fill: Color32::from_rgb(70, 130, 215),
                stroke: Stroke::new(1.0, Color32::from_rgb(90, 150, 235)),
            },
            
            // Hyperlinks
            hyperlink_color: Color32::from_rgb(100, 180, 255),
            
            // Window - darker background for better contrast
            window_fill: Color32::from_rgb(28, 30, 34),
            window_stroke: Stroke::new(1.0, Color32::from_rgb(48, 52, 58)),
            window_shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 10.0),
                blur: 20.0,
                spread: 0.0,
                color: Color32::from_black_alpha(120),
            },
            window_rounding: Rounding::same(10.0),
            
            // Panel
            panel_fill: Color32::from_rgb(32, 34, 38),
            
            // Popup
            popup_shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 6.0),
                blur: 16.0,
                spread: 0.0,
                color: Color32::from_black_alpha(140),
            },
            
            // Text colors - improved readability
            override_text_color: Some(Color32::from_rgb(225, 228, 232)),
            warn_fg_color: Color32::from_rgb(255, 180, 40),
            error_fg_color: Color32::from_rgb(255, 100, 100),
            
            // Other
            faint_bg_color: Color32::from_rgb(42, 45, 50),
            extreme_bg_color: Color32::from_rgb(20, 22, 25),
            code_bg_color: Color32::from_rgb(38, 40, 45),
            
            ..Visuals::dark()
        }
    }
    
    fn light_theme() -> Visuals {
        Visuals {
            dark_mode: false,

            // Modern light theme with better contrast
            widgets: egui::style::Widgets {
                noninteractive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(248),
                    weak_bg_fill: Color32::from_gray(248),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(200)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_gray(30)),
                    expansion: 0.0,
                },
                inactive: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(232),
                    weak_bg_fill: Color32::from_gray(232),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(185)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_gray(35)),
                    expansion: 0.0,
                },
                hovered: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(220),
                    weak_bg_fill: Color32::from_gray(220),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(165)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.5, Color32::BLACK),
                    expansion: 1.0,
                },
                active: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_rgb(85, 145, 215),
                    weak_bg_fill: Color32::from_rgb(85, 145, 215),
                    bg_stroke: Stroke::new(1.0, Color32::from_rgb(55, 115, 185)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(2.0, Color32::WHITE),
                    expansion: 1.0,
                },
                open: egui::style::WidgetVisuals {
                    bg_fill: Color32::from_gray(234),
                    weak_bg_fill: Color32::from_gray(234),
                    bg_stroke: Stroke::new(1.0, Color32::from_gray(180)),
                    rounding: Rounding::same(8.0),
                    fg_stroke: Stroke::new(1.0, Color32::from_gray(30)),
                    expansion: 0.0,
                },
            },

            selection: egui::style::Selection {
                bg_fill: Color32::from_rgb(85, 145, 215),
                stroke: Stroke::new(1.0, Color32::from_rgb(55, 115, 185)),
            },

            hyperlink_color: Color32::from_rgb(30, 90, 190),

            window_fill: Color32::from_gray(242),
            window_stroke: Stroke::new(1.0, Color32::from_gray(200)),
            window_shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 10.0),
                blur: 20.0,
                spread: 0.0,
                color: Color32::from_black_alpha(40),
            },
            window_rounding: Rounding::same(10.0),

            panel_fill: Color32::from_gray(250),

            override_text_color: Some(Color32::from_gray(25)),
            warn_fg_color: Color32::from_rgb(170, 90, 0),
            error_fg_color: Color32::from_rgb(200, 30, 30),

            faint_bg_color: Color32::from_gray(232),
            extreme_bg_color: Color32::from_gray(210),
            code_bg_color: Color32::from_gray(225),

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
