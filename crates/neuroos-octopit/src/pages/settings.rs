//! The Settings page: appearance, motion and about information (docs/design/octopit-ux.md §4.6).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_octopit_core::settings::{Settings, ThemeChoice};
use neuroos_octopit_core::state::{AppState, Changed};

use super::{Ctx, Page};
use crate::widgets::{card, label};

/// Callback invoked when settings are changed on the page.
pub type SettingsCallback = Box<dyn Fn(&Settings)>;

type CallbackList = Rc<RefCell<Vec<SettingsCallback>>>;

/// The Settings page.
#[allow(dead_code)]
pub struct SettingsPage {
    root: gtk::ScrolledWindow,
    settings: Rc<RefCell<Settings>>,
    callbacks: CallbackList,
    updating: Rc<Cell<bool>>,

    // Theme toggle buttons
    btn_system: gtk::ToggleButton,
    btn_light: gtk::ToggleButton,
    btn_dark: gtk::ToggleButton,

    // Text scale toggle buttons
    btn_100: gtk::ToggleButton,
    btn_115: gtk::ToggleButton,
    btn_130: gtk::ToggleButton,

    // Motion switch
    switch_motion: gtk::Switch,
}

impl SettingsPage {
    /// Builds the page.
    #[must_use]
    pub fn new(_ctx: &Ctx) -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.add_css_class("page");

        // Header
        content.append(&label("Settings", &["title-page"]));

        let settings = Rc::new(RefCell::new(Settings::default()));
        let callbacks: CallbackList = Rc::new(RefCell::new(Vec::new()));
        let updating = Rc::new(Cell::new(false));

        // Group 1: Appearance
        let app_card = card(&["settings-group"]);
        app_card.append(&label("Appearance", &["title-section"]));

        // Theme row
        let theme_section = gtk::Box::new(gtk::Orientation::Vertical, 6);
        theme_section.append(&label("Theme", &["title-card"]));

        let theme_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        theme_box.add_css_class("linked");

        let btn_system = gtk::ToggleButton::with_label("System");
        btn_system.add_css_class("btn");
        btn_system.update_property(&[gtk::accessible::Property::Label("System theme")]);
        btn_system.set_active(true);

        let btn_light = gtk::ToggleButton::with_label("Light");
        btn_light.add_css_class("btn");
        btn_light.update_property(&[gtk::accessible::Property::Label("Light theme")]);
        btn_light.set_group(Some(&btn_system));

        let btn_dark = gtk::ToggleButton::with_label("Dark");
        btn_dark.add_css_class("btn");
        btn_dark.update_property(&[gtk::accessible::Property::Label("Dark theme")]);
        btn_dark.set_group(Some(&btn_system));

        theme_box.append(&btn_system);
        theme_box.append(&btn_light);
        theme_box.append(&btn_dark);
        theme_section.append(&theme_box);
        app_card.append(&theme_section);

        // Text size row
        let text_section = gtk::Box::new(gtk::Orientation::Vertical, 6);
        text_section.add_css_class("settings-row");
        text_section.append(&label("Text size", &["title-card"]));

        let text_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        text_box.add_css_class("linked");

        let btn_100 = gtk::ToggleButton::with_label("100%");
        btn_100.add_css_class("btn");
        btn_100.update_property(&[gtk::accessible::Property::Label("Default text size 100%")]);
        btn_100.set_active(true);

        let btn_115 = gtk::ToggleButton::with_label("115%");
        btn_115.add_css_class("btn");
        btn_115.update_property(&[gtk::accessible::Property::Label("Medium text size 115%")]);
        btn_115.set_group(Some(&btn_100));

        let btn_130 = gtk::ToggleButton::with_label("130%");
        btn_130.add_css_class("btn");
        btn_130.update_property(&[gtk::accessible::Property::Label("Large text size 130%")]);
        btn_130.set_group(Some(&btn_100));

        text_box.append(&btn_100);
        text_box.append(&btn_115);
        text_box.append(&btn_130);
        text_section.append(&text_box);
        app_card.append(&text_section);

        content.append(&app_card);

        // Group 2: Motion
        let motion_card = card(&["settings-group"]);
        motion_card.append(&label("Motion", &["title-section"]));

        let motion_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        motion_row.set_valign(gtk::Align::Center);

        let motion_texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        motion_texts.set_hexpand(true);
        motion_texts.append(&label("Reduce motion", &["body"]));
        let motion_caption = label(
            "Stops the listening pulse and page fades. Also follows your desktop setting.",
            &["caption", "muted"],
        );
        motion_caption.set_wrap(true);
        motion_texts.append(&motion_caption);
        motion_row.append(&motion_texts);

        let switch_motion = gtk::Switch::new();
        switch_motion.set_valign(gtk::Align::Center);
        switch_motion.update_property(&[gtk::accessible::Property::Label("Reduce motion")]);
        motion_row.append(&switch_motion);

        motion_card.append(&motion_row);
        content.append(&motion_card);

        // Group 3: About
        let about_card = card(&["settings-group"]);
        about_card.append(&label("About", &["title-section"]));

        let version_label = label(
            &format!("OctoPit v{}", env!("CARGO_PKG_VERSION")),
            &["body", "mono"],
        );
        about_card.append(&version_label);

        let about_desc = label(
            "OctoPit is part of Semantic-NeuroOS. It reads status and sends your typed questions; it cannot approve anything.",
            &["caption"],
        );
        about_desc.set_wrap(true);
        about_card.append(&about_desc);

        let licence_label = label("Licence: GPL-3.0-only", &["caption", "muted"]);
        about_card.append(&licence_label);

        content.append(&about_card);

        // Wire event handlers to notify callbacks
        let notify = {
            let settings = Rc::clone(&settings);
            let callbacks = Rc::clone(&callbacks);
            let updating = Rc::clone(&updating);
            move || {
                if !updating.get() {
                    let cur = settings.borrow().clone();
                    for cb in callbacks.borrow().iter() {
                        cb(&cur);
                    }
                }
            }
        };

        // Theme handlers
        {
            let s = Rc::clone(&settings);
            let n = notify.clone();
            btn_system.connect_toggled(move |btn| {
                if btn.is_active() {
                    s.borrow_mut().theme = ThemeChoice::System;
                    n();
                }
            });
        }
        {
            let s = Rc::clone(&settings);
            let n = notify.clone();
            btn_light.connect_toggled(move |btn| {
                if btn.is_active() {
                    s.borrow_mut().theme = ThemeChoice::Light;
                    n();
                }
            });
        }
        {
            let s = Rc::clone(&settings);
            let n = notify.clone();
            btn_dark.connect_toggled(move |btn| {
                if btn.is_active() {
                    s.borrow_mut().theme = ThemeChoice::Dark;
                    n();
                }
            });
        }

        // Text scale handlers
        {
            let s = Rc::clone(&settings);
            let n = notify.clone();
            btn_100.connect_toggled(move |btn| {
                if btn.is_active() {
                    s.borrow_mut().text_scale = 100;
                    n();
                }
            });
        }
        {
            let s = Rc::clone(&settings);
            let n = notify.clone();
            btn_115.connect_toggled(move |btn| {
                if btn.is_active() {
                    s.borrow_mut().text_scale = 115;
                    n();
                }
            });
        }
        {
            let s = Rc::clone(&settings);
            let n = notify.clone();
            btn_130.connect_toggled(move |btn| {
                if btn.is_active() {
                    s.borrow_mut().text_scale = 130;
                    n();
                }
            });
        }

        // Motion switch handler
        {
            let s = Rc::clone(&settings);
            let n = notify;
            switch_motion.connect_active_notify(move |sw| {
                s.borrow_mut().reduce_motion = sw.is_active();
                n();
            });
        }

        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&content));
        root.set_hscrollbar_policy(gtk::PolicyType::Automatic);

        Self {
            root,
            settings,
            callbacks,
            updating,
            btn_system,
            btn_light,
            btn_dark,
            btn_100,
            btn_115,
            btn_130,
            switch_motion,
        }
    }

    /// Registers a callback to be called whenever settings are modified on the page.
    #[allow(dead_code)]
    pub fn on_change(&self, f: Box<dyn Fn(&Settings)>) {
        self.callbacks.borrow_mut().push(f);
    }

    /// Updates the UI widgets from `s` without triggering `on_change` callbacks.
    #[allow(dead_code)]
    pub fn set(&self, s: &Settings) {
        *self.settings.borrow_mut() = s.clone();
        self.updating.set(true);

        match s.theme {
            ThemeChoice::System => self.btn_system.set_active(true),
            ThemeChoice::Light => self.btn_light.set_active(true),
            ThemeChoice::Dark => self.btn_dark.set_active(true),
        }

        match s.text_scale {
            115 => self.btn_115.set_active(true),
            130 => self.btn_130.set_active(true),
            _ => self.btn_100.set_active(true),
        }

        self.switch_motion.set_active(s.reduce_motion);
        self.updating.set(false);
    }
}

impl Page for SettingsPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn refresh(&self, _state: &AppState, _changed: Changed) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_initial_state() {
        let initial = Settings::default();
        assert_eq!(initial.theme, ThemeChoice::System);
        assert!(!initial.reduce_motion);
        assert_eq!(initial.text_scale, 100);
        assert!(!initial.welcome_done);
    }
}
