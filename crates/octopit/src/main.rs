//! `octopit`: the desktop dashboard for OctoPlus (ADR-0017).
//!
//! A client like `octoplusctl`: it talks to the parts over the sockets the socket map lists for
//! it, shows status and chat, and can never approve anything.

#![allow(clippy::print_stdout, clippy::print_stderr)] // a command line program: --version, --help and the screenshot tool print

mod app;
mod charts;
mod mic;
mod pages;
mod palette;
mod shots;
mod theme;
mod tokens {
    include!(concat!(env!("OUT_DIR"), "/tokens.rs"));
}
mod welcome;
mod widgets;

use std::path::PathBuf;
use std::process::ExitCode;

use gtk::prelude::*;
use gtk4 as gtk;

use octopit_core::controller::{self, Rates};
use octoplus_common::paths;

fn runtime_root() -> Option<PathBuf> {
    paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref()).ok()
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("octopit {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "octopit: the OctoPlus dashboard\n\nUsage: octopit [--version] [--screenshots DIR]"
        );
        return ExitCode::SUCCESS;
    }
    let Some(root) = runtime_root() else {
        eprintln!("XDG_RUNTIME_DIR is not set: is this a desktop session?");
        return ExitCode::FAILURE;
    };
    let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    else {
        eprintln!("could not start the background runtime");
        return ExitCode::FAILURE;
    };
    let (controller, updates) = controller::start(rt.handle(), &root, Rates::default());
    let updates = std::cell::RefCell::new(Some(updates));
    let shots_dir = args
        .iter()
        .position(|a| a == "--screenshots")
        .and_then(|i| args.get(i + 1).cloned());

    let application = gtk::Application::builder()
        .application_id("org.octoplus.OctoPit")
        // The screenshot review must not hand over to a window that is already open.
        .flags(if shots_dir.is_some() {
            gtk::gio::ApplicationFlags::NON_UNIQUE
        } else {
            gtk::gio::ApplicationFlags::default()
        })
        .build();
    application.connect_activate(move |app| {
        if let Some(rx) = updates.borrow_mut().take() {
            let win = app::Window::build(app, &controller, rx);
            win.window.present();
            if let Some(dir) = shots_dir.clone() {
                shots::run(&win, dir, app);
            }
        } else if let Some(w) = app.active_window() {
            w.present();
        }
    });
    let code = application.run_with_args::<&str>(&[]);
    drop(rt);
    if code == gtk::glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
