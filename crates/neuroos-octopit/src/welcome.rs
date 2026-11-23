//! The First-Run Welcome window (docs/design/octopit-ux.md §4.7).

#![allow(dead_code)]

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use crate::widgets::{icon, label, plain_text};

const STEPS: [(&str, &str, &str); 3] = [
    (
        "octopit-overview-symbolic",
        "See everything",
        "Health, models, memory and the GPU, live, on one screen.",
    ),
    (
        "octopit-mic-symbolic",
        "Talk or type",
        "Use the microphone button or the Chat page. Everything is answered on this machine.",
    ),
    (
        "octopit-privacy-symbolic",
        "What OctoPit never does",
        "It cannot approve anything: approvals happen only in the dialog. It never records or hears audio. It never uses the network.",
    ),
];

/// Shows the first-run welcome dialog modal to `parent`. Calls `on_done` upon completion or skip.
pub fn show(parent: &gtk::ApplicationWindow, on_done: Box<dyn Fn()>) {
    let window = gtk::Window::new();
    window.add_css_class("octopit");
    window.set_transient_for(Some(parent));
    window.set_modal(true);
    window.set_title(Some("Welcome to OctoPit"));
    window.set_default_size(560, 420);
    window.set_resizable(false);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.add_css_class("welcome-window");

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(200);
    stack.set_vexpand(true);

    for (i, &(icon_name, heading, body)) in STEPS.iter().enumerate() {
        let step_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        step_box.add_css_class("welcome-step");
        step_box.set_valign(gtk::Align::Center);
        step_box.set_halign(gtk::Align::Center);

        let img = icon(icon_name, 32);
        step_box.append(&img);

        let head_lbl = label(heading, &["title-section"]);
        head_lbl.set_halign(gtk::Align::Center);
        step_box.append(&head_lbl);

        let body_lbl = plain_text(body);
        body_lbl.set_selectable(false);
        body_lbl.set_justify(gtk::Justification::Center);
        body_lbl.add_css_class("body");
        body_lbl.set_xalign(0.5);
        body_lbl.set_justify(gtk::Justification::Center);
        body_lbl.set_max_width_chars(42);
        step_box.append(&body_lbl);

        stack.add_named(&step_box, Some(&format!("step_{i}")));
    }
    content.append(&stack);

    // Button bar
    let btn_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    btn_bar.set_valign(gtk::Align::End);

    let skip_btn = gtk::Button::with_label("Skip");
    skip_btn.add_css_class("btn");
    skip_btn.add_css_class("flat");
    skip_btn.update_property(&[gtk::accessible::Property::Label("Skip welcome tour")]);
    btn_bar.append(&skip_btn);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    btn_bar.append(&spacer);

    let back_btn = gtk::Button::with_label("Back");
    back_btn.add_css_class("btn");
    back_btn.update_property(&[gtk::accessible::Property::Label("Previous step")]);
    back_btn.set_visible(false);
    btn_bar.append(&back_btn);

    let next_btn = gtk::Button::with_label("Next");
    next_btn.add_css_class("btn");
    next_btn.add_css_class("primary");
    next_btn.update_property(&[gtk::accessible::Property::Label("Next step")]);
    btn_bar.append(&next_btn);

    content.append(&btn_bar);
    window.set_child(Some(&content));

    let current_step = Rc::new(Cell::new(0usize));
    let done_called = Rc::new(Cell::new(false));
    let on_done = Rc::new(on_done);

    let finish = {
        let done_called = Rc::clone(&done_called);
        let on_done = Rc::clone(&on_done);
        let win = window.clone();
        move || {
            if !done_called.get() {
                done_called.set(true);
                on_done();
            }
            win.close();
        }
    };

    let update_ui = {
        let stack = stack.clone();
        let back_btn = back_btn.clone();
        let next_btn = next_btn.clone();
        let current_step = Rc::clone(&current_step);
        move || {
            let step = current_step.get();
            stack.set_visible_child_name(&format!("step_{step}"));
            back_btn.set_visible(step > 0);
            if step >= STEPS.len() - 1 {
                next_btn.set_label("Get started");
                next_btn.update_property(&[gtk::accessible::Property::Label(
                    "Finish welcome tour and get started",
                )]);
            } else {
                next_btn.set_label("Next");
                next_btn.update_property(&[gtk::accessible::Property::Label("Next step")]);
            }
        }
    };

    // Skip button clicked
    {
        let finish = finish.clone();
        skip_btn.connect_clicked(move |_| {
            finish();
        });
    }

    // Back button clicked
    {
        let current_step = Rc::clone(&current_step);
        let update_ui = update_ui.clone();
        back_btn.connect_clicked(move |_| {
            let step = current_step.get();
            if step > 0 {
                current_step.set(step - 1);
                update_ui();
            }
        });
    }

    // Next button clicked
    {
        let current_step = Rc::clone(&current_step);
        let update_ui = update_ui.clone();
        let finish = finish.clone();
        next_btn.connect_clicked(move |_| {
            let step = current_step.get();
            if step + 1 < STEPS.len() {
                current_step.set(step + 1);
                update_ui();
            } else {
                finish();
            }
        });
    }

    // Escape or window close request
    {
        let finish_close = finish.clone();
        window.connect_close_request(move |_| {
            finish_close();
            gtk::glib::Propagation::Proceed
        });
    }

    {
        let key_ctrl = gtk::EventControllerKey::new();
        let finish_esc = finish;
        key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape {
                finish_esc();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        window.add_controller(key_ctrl);
    }

    update_ui();
    window.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_steps_metadata() {
        assert_eq!(STEPS.len(), 3);
        assert_eq!(STEPS[0].1, "See everything");
        assert_eq!(STEPS[1].1, "Talk or type");
        assert_eq!(STEPS[2].1, "What OctoPit never does");
        for step in STEPS {
            assert!(!step.0.is_empty());
            assert!(!step.1.is_empty());
            assert!(!step.2.is_empty());
        }
    }
}
