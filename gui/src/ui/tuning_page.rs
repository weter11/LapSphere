use gtk::prelude::*;
use gtk::{Box, Button, DrawingArea, Orientation, Scale, ScrolledWindow};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::Config;
use crate::dbus_client::DbusClient;

const GRAPH_WIDTH: f64 = 500.0;
const GRAPH_HEIGHT: f64 = 300.0;
const MARGIN: f64 = 40.0;

pub fn create_page(
    config: Rc<RefCell<Config>>,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) -> ScrolledWindow {
    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    
    let main_box = Box::new(Orientation::Vertical, 12);
    main_box.set_margin_top(24);
    main_box.set_margin_bottom(24);
    main_box.set_margin_start(24);
    main_box.set_margin_end(24);
    
    let current_profile_name = config.borrow().data.current_profile.clone();
    let current_profile = config.borrow().data.profiles.iter()
        .find(|p| p.name == current_profile_name)
        .cloned();
    
    if let Some(profile) = current_profile {
        for section_name in &config.borrow().data.tuning_section_order {
            match section_name.as_str() {
                "Keyboard" => {
                    let section = create_keyboard_section(&profile, config.clone(), dbus_client.clone());
                    main_box.append(&section);
                }
                "CPU" => {
                    let section = create_cpu_section(&profile, config.clone(), dbus_client.clone());
                    main_box.append(&section);
                }
                "GPU" => {
                    let section = create_gpu_section(&profile);
                    main_box.append(&section);
                }
                "Screen" => {
                    let section = create_screen_section(&profile);
                    main_box.append(&section);
                }
                "Fans" => {
                    let section = create_fans_section_graphical(&profile, config.clone());
                    main_box.append(&section);
                }
                _ => {}
            }
        }
        
        let button_box = Box::new(Orientation::Horizontal, 6);
        button_box.set_halign(gtk::Align::End);
        button_box.set_margin_top(12);
        
        let apply_button = gtk::Button::with_label("Apply & Save");
        apply_button.set_css_classes(&["suggested-action"]);
        
        let conf_clone = config.clone();
        let dbus_clone = dbus_client.clone();
        apply_button.connect_clicked(move |_| {
            let _ = conf_clone.borrow().save();
            
            let profile_name = conf_clone.borrow().data.current_profile.clone();
            if let Some(profile) = conf_clone.borrow().data.profiles.iter()
                .find(|p| p.name == profile_name) {
                
                if let Some(client) = dbus_clone.borrow().as_ref() {
                    match client.apply_profile(&profile) {
                        Ok(_) => println!("Profile applied successfully"),
                        Err(e) => eprintln!("Failed to apply profile: {}", e),
                    }
                }
            }
        });
        
        button_box.append(&apply_button);
        main_box.append(&button_box);
    }
    
    scrolled.set_child(Some(&main_box));
    scrolled
}

fn create_fans_section_graphical(
    profile: &tuxedo_common::types::Profile,
    config: Rc<RefCell<Config>>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Fan Control")
        .build();
    
    let control_row = adw::SwitchRow::builder()
        .title("Manual fan control")
        .subtitle("Use custom fan curves (disable for auto mode)")
        .build();
    
    control_row.set_active(profile.fan_settings.control_enabled);
    group.add(&control_row);
    
    if profile.fan_settings.control_enabled {
        for curve in profile.fan_settings.curves.iter() {
            let curve_expander = adw::ExpanderRow::builder()
                .title(&format!("Fan {} Curve Editor", curve.fan_id))
                .subtitle(&format!("{} control points", curve.points.len()))
                .build();
            
            // Create graphical editor
            let editor_box = Box::new(Orientation::Vertical, 12);
            editor_box.set_margin_top(12);
            editor_box.set_margin_bottom(12);
            
            // Drawing area
            let drawing_area = DrawingArea::new();
            drawing_area.set_content_width(GRAPH_WIDTH as i32);
            drawing_area.set_content_height(GRAPH_HEIGHT as i32);
            
            let points = Rc::new(RefCell::new(curve.points.clone()));
            let selected = Rc::new(RefCell::new(None::<usize>));
            
            // Draw function
            let points_draw = points.clone();
            let selected_draw = selected.clone();
            drawing_area.set_draw_func(move |_, cr, _, _| {
                draw_fan_curve(cr, &points_draw.borrow(), *selected_draw.borrow());
            });
            
            // Mouse click to select points
            let gesture = gtk::GestureClick::new();
            let points_click = points.clone();
            let selected_click = selected.clone();
            let da_click = drawing_area.clone();
            gesture.connect_pressed(move |_, _, x, y| {
                let mut sel = selected_click.borrow_mut();
                let pts = points_click.borrow();
                
                let mut closest_idx = None;
                let mut closest_dist = f64::MAX;
                
                for (i, (temp, speed)) in pts.iter().enumerate() {
                    let px = MARGIN + (*temp as f64 / 100.0) * (GRAPH_WIDTH - 2.0 * MARGIN);
                    let py = GRAPH_HEIGHT - MARGIN - (*speed as f64 / 100.0) * (GRAPH_HEIGHT - 2.0 * MARGIN);
                    
                    let dist = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                    if dist < 20.0 && dist < closest_dist {
                        closest_dist = dist;
                        closest_idx = Some(i);
                    }
                }
                
                *sel = closest_idx;
                da_click.queue_draw();
            });
            drawing_area.add_controller(gesture);
            
            // Drag to move points
            let drag = gtk::GestureDrag::new();
            let points_drag = points.clone();
            let selected_drag = selected.clone();
            let da_drag = drawing_area.clone();
            drag.connect_drag_update(move |_, x, y| {
                if let Some(idx) = *selected_drag.borrow() {
                    let mut pts = points_drag.borrow_mut();
                    if let Some((temp, speed)) = pts.get_mut(idx) {
                        let graph_width = GRAPH_WIDTH - 2.0 * MARGIN;
                        let graph_height = GRAPH_HEIGHT - 2.0 * MARGIN;
                        
                        let base_x = MARGIN + (*temp as f64 / 100.0) * graph_width;
                        let new_x = (base_x + x).clamp(MARGIN, GRAPH_WIDTH - MARGIN);
                        let new_temp = ((new_x - MARGIN) / graph_width * 100.0) as u8;
                        
                        let base_y = GRAPH_HEIGHT - MARGIN - (*speed as f64 / 100.0) * graph_height;
                        let new_y = (base_y + y).clamp(MARGIN, GRAPH_HEIGHT - MARGIN);
                        let new_speed = ((GRAPH_HEIGHT - MARGIN - new_y) / graph_height * 100.0) as u8;
                        
                        *temp = new_temp;
                        *speed = new_speed.clamp(0, 100);
                    }
                    da_drag.queue_draw();
                }
            });
            drawing_area.add_controller(drag);
            
            editor_box.append(&drawing_area);
            
            // Control buttons
            let btn_box = Box::new(Orientation::Horizontal, 6);
            btn_box.set_halign(gtk::Align::Center);
            
            let add_btn = Button::with_label("➕ Add Point");
            let points_add = points.clone();
            let da_add = drawing_area.clone();
            add_btn.connect_clicked(move |_| {
                let mut pts = points_add.borrow_mut();
                if pts.len() < 8 {
                    pts.push((50, 50));
                    pts.sort_by_key(|(t, _)| *t);
                    da_add.queue_draw();
                }
            });
            btn_box.append(&add_btn);
            
            let remove_btn = Button::with_label("➖ Remove");
            let points_remove = points.clone();
            let selected_remove = selected.clone();
            let da_remove = drawing_area.clone();
            remove_btn.connect_clicked(move |_| {
                if let Some(idx) = *selected_remove.borrow() {
                    let mut pts = points_remove.borrow_mut();
                    if pts.len() > 2 {
                        pts.remove(idx);
                        *selected_remove.borrow_mut() = None;
                        da_remove.queue_draw();
                    }
                }
            });
            btn_box.append(&remove_btn);
            
            let save_btn = Button::with_label("Save Curve");
            save_btn.set_css_classes(&["suggested-action"]);
            let points_save = points.clone();
            let config_save = config.clone();
            let fan_id = curve.fan_id;
            save_btn.connect_clicked(move |_| {
                let pts = points_save.borrow().clone();
                let mut cfg = config_save.borrow_mut();
                let profile_name = cfg.data.current_profile.clone();
                
                if let Some(prof) = cfg.data.profiles.iter_mut().find(|p| p.name == profile_name) {
                    if let Some(crv) = prof.fan_settings.curves.iter_mut().find(|c| c.fan_id == fan_id) {
                        crv.points = pts;
                        let _ = cfg.save();
                        println!("Fan curve saved");
                    }
                }
            });
            btn_box.append(&save_btn);
            
            editor_box.append(&btn_box);
            
            // Info
            let info = gtk::Label::new(Some("Click to select • Drag to move • Temperature: 0-100°C, Speed: 0-100%"));
            info.set_wrap(true);
            info.set_css_classes(&["dim-label", "caption"]);
            editor_box.append(&info);
            
            let wrapper_row = adw::ActionRow::new();
            wrapper_row.set_child(Some(&editor_box));
            curve_expander.add_row(&wrapper_row);
            
            group.add(&curve_expander);
        }
    }
    
    group
}

fn draw_fan_curve(cr: &gtk::cairo::Context, points: &[(u8, u8)], selected: Option<usize>) {
    let w = GRAPH_WIDTH;
    let h = GRAPH_HEIGHT;
    
    // Background
    cr.set_source_rgb(0.97, 0.97, 0.97);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    
    // Graph area
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(MARGIN, MARGIN, w - 2.0 * MARGIN, h - 2.0 * MARGIN);
    let _ = cr.fill();
    
    // Grid
    cr.set_source_rgba(0.85, 0.85, 0.85, 0.5);
    cr.set_line_width(1.0);
    for i in 0..=10 {
        let x = MARGIN + (i as f64 / 10.0) * (w - 2.0 * MARGIN);
        cr.move_to(x, MARGIN);
        cr.line_to(x, h - MARGIN);
        let _ = cr.stroke();
        
        let y = MARGIN + (i as f64 / 10.0) * (h - 2.0 * MARGIN);
        cr.move_to(MARGIN, y);
        cr.line_to(w - MARGIN, y);
        let _ = cr.stroke();
    }
    
    // Border
    cr.set_source_rgb(0.2, 0.2, 0.2);
    cr.set_line_width(2.0);
    cr.rectangle(MARGIN, MARGIN, w - 2.0 * MARGIN, h - 2.0 * MARGIN);
    let _ = cr.stroke();
    
    // Labels
    cr.set_font_size(11.0);
    cr.move_to(w / 2.0 - 50.0, h - 8.0);
    let _ = cr.show_text("Temperature (°C)");
    
    cr.save().unwrap();
    cr.translate(12.0, h / 2.0 + 35.0);
    cr.rotate(-std::f64::consts::PI / 2.0);
    let _ = cr.show_text("Fan Speed (%)");
    cr.restore().unwrap();
    
    if points.is_empty() {
        return;
    }
    
    // Curve line
    cr.set_source_rgb(0.2, 0.5, 0.9);
    cr.set_line_width(2.5);
    
    for (i, (temp, speed)) in points.iter().enumerate() {
        let x = MARGIN + (*temp as f64 / 100.0) * (w - 2.0 * MARGIN);
        let y = h - MARGIN - (*speed as f64 / 100.0) * (h - 2.0 * MARGIN);
        
        if i == 0 {
            cr.move_to(x, y);
        } else {
            cr.line_to(x, y);
        }
    }
    let _ = cr.stroke();
    
    // Points
    for (i, (temp, speed)) in points.iter().enumerate() {
        let x = MARGIN + (*temp as f64 / 100.0) * (w - 2.0 * MARGIN);
        let y = h - MARGIN - (*speed as f64 / 100.0) * (h - 2.0 * MARGIN);
        
        if Some(i) == selected {
            cr.set_source_rgb(0.9, 0.2, 0.2);
        } else {
            cr.set_source_rgb(0.2, 0.5, 0.9);
        }
        cr.arc(x, y, 5.0, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
        
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(1.5);
        cr.arc(x, y, 5.0, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();
        
        // Label
        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.set_font_size(9.0);
        cr.move_to(x + 8.0, y - 8.0);
        let _ = cr.show_text(&format!("{}°C", temp));
        cr.move_to(x + 8.0, y + 2.0);
        let _ = cr.show_text(&format!("{}%", speed));
    }
}

fn create_keyboard_section(
    profile: &tuxedo_common::types::Profile,
    _config: Rc<RefCell<Config>>,
    _dbus_client: Rc<RefCell<Option<DbusClient>>>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Keyboard Backlight")
        .build();
    
    let control_row = adw::SwitchRow::builder()
        .title("Manual control")
        .subtitle("When disabled, system controls the backlight")
        .build();
    control_row.set_active(profile.keyboard_settings.control_enabled);
    group.add(&control_row);
    
    if let tuxedo_common::types::KeyboardMode::SingleColor { r, g, b, brightness } = profile.keyboard_settings.mode {
        let red_row = adw::ActionRow::builder()
            .title("Red")
            .subtitle(&format!("{}", r))
            .build();
        
        let red_scale = Scale::with_range(gtk::Orientation::Horizontal, 0.0, 255.0, 1.0);
        red_scale.set_value(r as f64);
        red_scale.set_hexpand(true);
        red_row.add_suffix(&red_scale);
        group.add(&red_row);
        
        let green_row = adw::ActionRow::builder()
            .title("Green")
            .subtitle(&format!("{}", g))
            .build();
        
        let green_scale = Scale::with_range(gtk::Orientation::Horizontal, 0.0, 255.0, 1.0);
        green_scale.set_value(g as f64);
        green_scale.set_hexpand(true);
        green_row.add_suffix(&green_scale);
        group.add(&green_row);
        
        let blue_row = adw::ActionRow::builder()
            .title("Blue")
            .subtitle(&format!("{}", b))
            .build();
        
        let blue_scale = Scale::with_range(gtk::Orientation::Horizontal, 0.0, 255.0, 1.0);
        blue_scale.set_value(b as f64);
        blue_scale.set_hexpand(true);
        blue_row.add_suffix(&blue_scale);
        group.add(&blue_row);
        
        let brightness_row = adw::ActionRow::builder()
            .title("Brightness")
            .subtitle(&format!("{}%", brightness))
            .build();
        
        let brightness_scale = Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        brightness_scale.set_value(brightness as f64);
        brightness_scale.set_hexpand(true);
        brightness_row.add_suffix(&brightness_scale);
        group.add(&brightness_row);
    }
    
    group
}

fn create_cpu_section(
    profile: &tuxedo_common::types::Profile,
    _config: Rc<RefCell<Config>>,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("CPU Settings")
        .description("Profile-specific CPU configuration")
        .build();
    
    let available_controls = if let Some(client) = dbus_client.borrow().as_ref() {
        if let Ok(cpu_info) = client.get_cpu_info() {
            cpu_info.available_pstate_controls
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    
    if available_controls.contains(&"scaling_governor".to_string()) {
        let cpu_info = if let Some(client) = dbus_client.borrow().as_ref() {
            client.get_cpu_info().ok()
        } else {
            None
        };
        
        if let Some(info) = cpu_info {
            let governor_row = adw::ComboRow::builder()
                .title("CPU Governor")
                .build();
            
            let governors: Vec<&str> = info.available_governors.iter().map(|s| s.as_str()).collect();
            let governor_model = gtk::StringList::new(&governors);
            governor_row.set_model(Some(&governor_model));
            
            if let Some(ref current_gov) = profile.cpu_settings.governor {
                if let Some(idx) = info.available_governors.iter().position(|g| g == current_gov) {
                    governor_row.set_selected(idx as u32);
                }
            }
            
            group.add(&governor_row);
        }
    }
    
    if available_controls.contains(&"boost".to_string()) {
        let boost_row = adw::SwitchRow::builder()
            .title("CPU Boost")
            .subtitle("Turbo / Precision Boost")
            .build();
        
        if let Some(boost) = profile.cpu_settings.boost {
            boost_row.set_active(boost);
        }
        
        group.add(&boost_row);
    }
    
    if available_controls.contains(&"smt".to_string()) {
        let smt_row = adw::SwitchRow::builder()
            .title("SMT / Hyperthreading")
            .subtitle("Simultaneous Multithreading")
            .build();
        
        if let Some(smt) = profile.cpu_settings.smt {
            smt_row.set_active(smt);
        }
        
        group.add(&smt_row);
    }
    
    if available_controls.contains(&"energy_performance_preference".to_string()) {
        if let Some(client) = dbus_client.borrow().as_ref() {
            if let Ok(cpu_info) = client.get_cpu_info() {
                if !cpu_info.available_epp_preferences.is_empty() {
                    let epp_row = adw::ComboRow::builder()
                        .title("Energy Performance Preference")
                        .subtitle("Balance between performance and power saving")
                        .build();
                    
                    let epp_prefs: Vec<&str> = cpu_info.available_epp_preferences.iter()
                        .map(|s| s.as_str()).collect();
                    let epp_model = gtk::StringList::new(&epp_prefs);
                    epp_row.set_model(Some(&epp_model));
                    
                    if let Some(ref current_epp) = profile.cpu_settings.energy_performance_preference {
                        if let Some(idx) = cpu_info.available_epp_preferences.iter()
                            .position(|e| e == current_epp) {
                            epp_row.set_selected(idx as u32);
                        }
                    }
                    
                    group.add(&epp_row);
                }
            }
        }
    }
    
    group
}

fn create_gpu_section(profile: &tuxedo_common::types::Profile) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("GPU Settings")
        .build();
    
    if let Some(ref tdp) = profile.gpu_settings.dgpu_tdp {
        let tdp_row = adw::ActionRow::builder()
            .title("dGPU TDP Limit")
            .subtitle(&format!("{} W", tdp))
            .build();
        group.add(&tdp_row);
    } else {
        let na_row = adw::ActionRow::builder()
            .title("dGPU TDP")
            .subtitle("Not supported on this hardware")
            .build();
        group.add(&na_row);
    }
    
    group
}

fn create_screen_section(profile: &tuxedo_common::types::Profile) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Screen Brightness")
        .build();
    
    let system_control_row = adw::SwitchRow::builder()
        .title("System control")
        .subtitle("Let the system manage brightness")
        .build();
    
    system_control_row.set_active(profile.screen_settings.system_control);
    group.add(&system_control_row);
    
    let brightness_row = adw::ActionRow::builder()
        .title("Brightness")
        .subtitle(&format!("{}%", profile.screen_settings.brightness))
        .build();
    
    let brightness_scale = Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    brightness_scale.set_value(profile.screen_settings.brightness as f64);
    brightness_scale.set_hexpand(true);
    brightness_scale.set_sensitive(!profile.screen_settings.system_control);
    brightness_row.add_suffix(&brightness_scale);
    group.add(&brightness_row);
    
    group
}
