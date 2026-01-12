use gtk::prelude::*;
use gtk::{Box, Button, Entry, Orientation, ScrolledWindow};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::Config;
use crate::dbus_client::DbusClient;
use tuxedo_common::types::Profile;

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
    
    // Current profile indicator
    let current_profile = &config.borrow().data.current_profile;
    let indicator = gtk::Label::new(Some(&format!("Current Profile: {}", current_profile)));
    indicator.set_css_classes(&["title-2"]);
    main_box.append(&indicator);
    
    // Profiles list
    let profiles_list = create_profiles_list(config.clone(), dbus_client.clone());
    main_box.append(&profiles_list);
    
    // Action buttons
    let button_box = Box::new(Orientation::Horizontal, 6);
    button_box.set_halign(gtk::Align::Start);
    button_box.set_margin_top(12);
    
    let new_button = Button::with_label("➕ New Profile");
    let new_config = config.clone();
    let new_dbus = dbus_client.clone();
    new_button.connect_clicked(move |_| {
        create_new_profile(new_config.clone(), new_dbus.clone());
    });
    button_box.append(&new_button);
    
    main_box.append(&button_box);
    
    scrolled.set_child(Some(&main_box));
    scrolled
}

fn create_profiles_list(
    config: Rc<RefCell<Config>>,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) -> Box {
    let list_box = Box::new(Orientation::Vertical, 12);
    
    let profiles = config.borrow().data.profiles.clone();
    
    for profile in profiles {
        let profile_card = create_profile_card(&profile, config.clone(), dbus_client.clone());
        list_box.append(&profile_card);
    }
    
    list_box
}

fn create_profile_card(
    profile: &Profile,
    config: Rc<RefCell<Config>>,
    dbus_client: Rc<RefCell<Option<DbusClient>>>,
) -> adw::ExpanderRow {
    let expander = adw::ExpanderRow::builder()
        .title(&profile.name)
        .subtitle(if profile.is_default { "Default Profile" } else { "Custom Profile" })
        .build();
    
    // Radio button for selection
    let radio = gtk::CheckButton::new();
    radio.set_active(&config.borrow().data.current_profile == &profile.name);
    expander.add_prefix(&radio);
    
    // Apply profile immediately when selected
    let profile_name = profile.name.clone();
    let config_clone = config.clone();
    let dbus_clone = dbus_client.clone();
    let profile_clone = profile.clone();
    radio.connect_toggled(move |rb| {
        if rb.is_active() {
            config_clone.borrow_mut().data.current_profile = profile_name.clone();
            let _ = config_clone.borrow().save();
            
            if let Some(client) = dbus_clone.borrow().as_ref() {
                match client.apply_profile(&profile_clone) {
                    Ok(_) => println!("Profile '{}' applied successfully", profile_name),
                    Err(e) => eprintln!("Failed to apply profile '{}': {}", profile_name, e),
                }
            }
        }
    });
    
    // CPU Settings
    let cpu_info = if let Some(ref gov) = profile.cpu_settings.governor {
        format!("CPU: {}", gov)
    } else {
        "CPU: Auto".to_string()
    };
    let cpu_row = adw::ActionRow::builder()
        .title("CPU Settings")
        .subtitle(&cpu_info)
        .build();
    expander.add_row(&cpu_row);
    
    // Fan Settings
    let fan_info = if profile.fan_settings.control_enabled {
        format!("Fans: Manual ({} curves)", profile.fan_settings.curves.len())
    } else {
        "Fans: Auto".to_string()
    };
    let fan_row = adw::ActionRow::builder()
        .title("Fan Settings")
        .subtitle(&fan_info)
        .build();
    expander.add_row(&fan_row);
    
    // Auto-Switch Settings
    let auto_switch_expander = adw::ExpanderRow::builder()
        .title("Auto-Switch Settings")
        .subtitle(if profile.auto_switch.enabled {
            format!("Enabled ({} apps)", profile.auto_switch.app_names.len())
        } else {
            "Disabled".to_string()
        })
        .build();
    
    // Enable/Disable switch
    let auto_switch_toggle = adw::SwitchRow::builder()
        .title("Enable Auto-Switch")
        .subtitle("Automatically activate this profile when specific apps are running")
        .build();
    auto_switch_toggle.set_active(profile.auto_switch.enabled);
    
    let profile_name_toggle = profile.name.clone();
    let config_toggle = config.clone();
    auto_switch_toggle.connect_active_notify(move |row| {
        let mut cfg = config_toggle.borrow_mut();
        if let Some(prof) = cfg.data.profiles.iter_mut().find(|p| p.name == profile_name_toggle) {
            prof.auto_switch.enabled = row.is_active();
            let _ = cfg.save();
        }
    });
    auto_switch_expander.add_row(&auto_switch_toggle);
    
    // App list
    let app_list_box = Box::new(Orientation::Vertical, 6);
    app_list_box.set_margin_top(12);
    app_list_box.set_margin_start(12);
    app_list_box.set_margin_end(12);
    
    for app_name in &profile.auto_switch.app_names {
        let app_box = Box::new(Orientation::Horizontal, 6);
        
        let app_label = gtk::Label::new(Some(app_name));
        app_label.set_hexpand(true);
        app_label.set_halign(gtk::Align::Start);
        app_box.append(&app_label);
        
        let remove_btn = Button::with_label("Remove");
        let app_to_remove = app_name.clone();
        let profile_name_remove = profile.name.clone();
        let config_remove = config.clone();
        remove_btn.connect_clicked(move |_| {
            let mut cfg = config_remove.borrow_mut();
            if let Some(prof) = cfg.data.profiles.iter_mut().find(|p| p.name == profile_name_remove) {
                prof.auto_switch.app_names.retain(|a| a != &app_to_remove);
                let _ = cfg.save();
            }
        });
        app_box.append(&remove_btn);
        
        app_list_box.append(&app_box);
    }
    
    // Add new app entry
    let add_app_box = Box::new(Orientation::Horizontal, 6);
    add_app_box.set_margin_top(6);
    
    let app_entry = Entry::new();
    app_entry.set_placeholder_text(Some("App process name (e.g., steam, chrome)"));
    app_entry.set_hexpand(true);
    add_app_box.append(&app_entry);
    
    let add_btn = Button::with_label("Add");
    add_btn.set_css_classes(&["suggested-action"]);
    let profile_name_add = profile.name.clone();
    let config_add = config.clone();
    let entry_clone = app_entry.clone();
    add_btn.connect_clicked(move |_| {
        let app_name = entry_clone.text().to_string();
        if !app_name.is_empty() {
            let mut cfg = config_add.borrow_mut();
            if let Some(prof) = cfg.data.profiles.iter_mut().find(|p| p.name == profile_name_add) {
                if !prof.auto_switch.app_names.contains(&app_name) {
                    prof.auto_switch.app_names.push(app_name);
                    let _ = cfg.save();
                    entry_clone.set_text("");
                }
            }
        }
    });
    add_app_box.append(&add_btn);
    
    app_list_box.append(&add_app_box);
    
    // Help text
    let help_label = gtk::Label::new(Some(
        "💡 Tip: Process names are found in /proc/[pid]/comm\n\
         Examples: steam, chrome, blender, gimp, code"
    ));
    help_label.set_wrap(true);
    help_label.set_css_classes(&["dim-label", "caption"]);
    help_label.set_halign(gtk::Align::Start);
    help_label.set_margin_top(6);
    app_list_box.append(&help_label);
    
    let app_list_row = adw::ActionRow::new();
    app_list_row.set_child(Some(&app_list_box));
    auto_switch_expander.add_row(&app_list_row);
    
    expander.add_row(&auto_switch_expander);
    
    // Edit and Delete buttons
    let actions_box = Box::new(Orientation::Horizontal, 6);
    
    let edit_button = Button::with_label("Edit");
    let edit_profile = profile.clone();
    edit_button.connect_clicked(move |_| {
        println!("Edit profile: {}", edit_profile.name);
    });
    actions_box.append(&edit_button);
    
    if !profile.is_default {
        let delete_button = Button::with_label("Delete");
        let del_profile_name = profile.name.clone();
        let del_config = config.clone();
        delete_button.connect_clicked(move |_| {
            delete_profile(&del_profile_name, del_config.clone());
        });
        actions_box.append(&delete_button);
    } else {
        let reset_button = Button::with_label("Reset to Stock");
        let reset_config = config.clone();
        reset_button.connect_clicked(move |_| {
            reset_default_profile(reset_config.clone());
        });
        actions_box.append(&reset_button);
    }
    
    expander.add_suffix(&actions_box);
    
    expander
}

fn create_new_profile(config: Rc<RefCell<Config>>, _dbus_client: Rc<RefCell<Option<DbusClient>>>) {
    let dialog = adw::MessageDialog::builder()
        .heading("Create New Profile")
        .body("Enter a name for the new profile:")
        .build();
    
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("create", "Create");
    dialog.set_default_response(Some("create"));
    dialog.set_close_response("cancel");
    
    let config_clone = config.clone();
    dialog.connect_response(Some("create"), move |_, _| {
        let profile_count = config_clone.borrow().data.profiles.len();
        let new_name = format!("Profile {}", profile_count);
        
        let default_profile = config_clone.borrow().data.profiles.iter()
            .find(|p| p.is_default)
            .cloned()
            .unwrap_or_default();
        
        let mut new_profile = default_profile;
        new_profile.name = new_name.clone();
        new_profile.is_default = false;
        
        config_clone.borrow_mut().data.profiles.push(new_profile);
        let _ = config_clone.borrow().save();
        
        println!("Created new profile: {}", new_name);
    });
    
    dialog.present();
}

fn delete_profile(profile_name: &str, config: Rc<RefCell<Config>>) {
    config.borrow_mut().data.profiles.retain(|p| p.name != profile_name);
    
    if config.borrow().data.current_profile == profile_name {
        config.borrow_mut().data.current_profile = "Default".to_string();
    }
    
    let _ = config.borrow().save();
    println!("Deleted profile: {}", profile_name);
}

fn reset_default_profile(config: Rc<RefCell<Config>>) {
    if let Some(profile) = config.borrow_mut().data.profiles.iter_mut().find(|p| p.is_default) {
        *profile = Profile::default();
    }
    
    let _ = config.borrow().save();
    println!("Reset default profile to stock settings");
}
