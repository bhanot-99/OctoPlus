//! `octopit --screenshots <dir>`: opens the window, lets real data arrive, then saves a PNG
//! of every page in both themes (the UX acceptance review, docs/design/octopit-ux.md §9). The
//! window is a real one on the user's desktop, so this is a manual or release-gate tool.

use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use crate::app::Window;
use crate::pages::PageId;
use crate::theme::Theme;

/// Pages shown in the review, in order.
const PAGES: [PageId; 7] = [
    PageId::Overview,
    PageId::Chat,
    PageId::Companion,
    PageId::Backends,
    PageId::Activity,
    PageId::Privacy,
    PageId::Settings,
];

/// Saves a shot, retrying while the window has not drawn a frame yet.
fn save_retry(win: &Rc<Window>, path: String, then: Box<dyn FnOnce()>, tries: u32) {
    match save(&win.window, &path) {
        Ok(()) => {
            println!("saved {path}");
            then();
        }
        Err(e) if tries > 0 => {
            let w = Rc::clone(win);
            glib::timeout_add_local_once(Duration::from_millis(800), move || {
                save_retry(&w, path, then, tries - 1);
            });
            let _ = e;
        }
        Err(e) => {
            eprintln!("screenshot {path}: {e}");
            then();
        }
    }
}

fn save(win: &impl IsA<gtk::Widget>, path: &str) -> Result<(), String> {
    let paintable = gtk::WidgetPaintable::new(Some(win));
    let snap = gtk::Snapshot::new();
    let (w, h) = (f64::from(win.width()), f64::from(win.height()));
    gtk::gdk::prelude::PaintableExt::snapshot(
        &paintable,
        snap.upcast_ref::<gtk::gdk::Snapshot>(),
        w,
        h,
    );
    let node = snap.to_node().ok_or_else(|| {
        format!(
            "empty snapshot (size {}x{}, mapped {})",
            win.width(),
            win.height(),
            win.is_mapped()
        )
    })?;
    let renderer = win
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    renderer
        .render_texture(node, None)
        .save_to_png(path)
        .map_err(|e| e.to_string())
}

type Steps = Rc<std::cell::RefCell<std::vec::IntoIter<(Theme, PageId)>>>;

/// Runs the whole review, then quits the application.
pub fn run(win: &Rc<Window>, dir: String, app: &gtk::Application) {
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {dir}: {e}");
        app.quit();
        return;
    }
    let mut steps: Vec<(Theme, PageId)> = Vec::new();
    for theme in [Theme::Dark, Theme::Light] {
        for page in PAGES {
            steps.push((theme, page));
        }
    }
    // A maximised window gives a roomy, repeatable size for the review.
    if std::env::var_os("OCTOPIT_SIZE").is_none() {
        win.window.maximize();
    }
    let steps: Steps = Rc::new(std::cell::RefCell::new(steps.into_iter()));
    let (w, app) = (Rc::clone(win), app.clone());
    // Give the first poll time to arrive, then one page at a time.
    glib::timeout_add_local_once(Duration::from_millis(2500), move || {
        next(w, steps, dir, app)
    });
}

fn next(w: Rc<Window>, steps: Steps, dir: String, app: gtk::Application) {
    let step = steps.borrow_mut().next();
    let Some((theme, page)) = step else {
        extras(w, dir, app);
        return;
    };
    w.set_theme(theme);
    w.show_page(page);
    // One real question through the whole stack, so the Chat shot shows a real answer.
    let wait = if page == PageId::Chat && theme == Theme::Dark {
        w.ask("What is two plus two? Answer in one short sentence.");
        25_000
    } else {
        1500
    };
    glib::timeout_add_local_once(Duration::from_millis(wait), move || {
        let name = format!(
            "{dir}/{}-{}.png",
            if theme == Theme::Dark {
                "dark"
            } else {
                "light"
            },
            page.key()
        );
        let w2 = Rc::clone(&w);
        save_retry(
            &w,
            name.clone(),
            Box::new(move || scroll_through(w2, name, 1, steps, dir, app)),
            4,
        );
    });
}

/// Pages down a tall page, saving `<name>-2.png`, `-3.png`, ... until the end (at most 6 shots).
#[allow(clippy::too_many_arguments)]
fn scroll_through(
    w: Rc<Window>,
    name: String,
    n: u32,
    steps: Steps,
    dir: String,
    app: gtk::Application,
) {
    if n < 6 && w.scroll_page_down() {
        let shot = name.replace(".png", &format!("-{}.png", n + 1));
        glib::timeout_add_local_once(Duration::from_millis(700), move || {
            let w2 = Rc::clone(&w);
            save_retry(
                &w,
                shot,
                Box::new(move || scroll_through(w2, name, n + 1, steps, dir, app)),
                4,
            );
        });
    } else {
        next(w, steps, dir, app);
    }
}

/// The palette and the welcome window are separate windows: save each one on its own.
fn extras(w: Rc<Window>, dir: String, app: gtk::Application) {
    w.set_theme(Theme::Dark);
    w.show_page(PageId::Overview);
    w.show_palette();
    glib::timeout_add_local_once(Duration::from_millis(800), move || {
        save_other_windows(&w, &format!("{dir}/palette.png"));
        for t in gtk::Window::list_toplevels() {
            if let Some(win) = t.downcast_ref::<gtk::Window>()
                && win.title().as_deref() == Some("Command palette")
            {
                win.close();
            }
        }
        crate::welcome::show(&w.window, Box::new(|| {}));
        glib::timeout_add_local_once(Duration::from_millis(800), move || {
            save_other_windows(&w, &format!("{dir}/welcome.png"));
            app.quit();
        });
    });
}

fn save_other_windows(main: &Rc<Window>, name: &str) {
    for t in gtk::Window::list_toplevels() {
        let Some(win) = t.downcast_ref::<gtk::Window>() else {
            continue;
        };
        if win == main.window.upcast_ref::<gtk::Window>() || !win.is_visible() {
            continue;
        }
        match save(win, name) {
            Ok(()) => println!("saved {name}"),
            Err(e) => eprintln!("screenshot {name}: {e}"),
        }
    }
}

type VerifySteps = Rc<std::cell::RefCell<std::vec::IntoIter<(String, String, String)>>>;

#[derive(Clone)]
struct VerifyCtx {
    steps: VerifySteps,
    dir: String,
    app: gtk::Application,
}

/// Interactive verification runner: drives the real ChatPage and CompanionPage from a script
/// file (`mode|shot_name|question`, or `companion|shot_name|`), waits for each reply to finish,
/// prints the exact output, and saves a PNG per step.
pub fn run_verify_chat(win: &Rc<Window>, dir: String, script_path: String, app: &gtk::Application) {
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {dir}: {e}");
        app.quit();
        return;
    }
    let Ok(raw) = std::fs::read_to_string(&script_path) else {
        eprintln!("cannot read {script_path}");
        app.quit();
        return;
    };
    let steps: Vec<(String, String, String)> = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.splitn(3, '|');
            Some((
                parts.next()?.trim().to_owned(),
                parts.next()?.trim().to_owned(),
                parts.next().unwrap_or("").trim().to_owned(),
            ))
        })
        .collect();
    let vctx = VerifyCtx {
        steps: Rc::new(std::cell::RefCell::new(steps.into_iter())),
        dir,
        app: app.clone(),
    };
    if std::env::var_os("OCTOPIT_SIZE").is_none() {
        win.window.maximize();
    }
    let w = Rc::clone(win);
    glib::timeout_add_local_once(Duration::from_millis(2500), move || {
        verify_next(w, vctx);
    });
}

fn verify_next(win: Rc<Window>, vctx: VerifyCtx) {
    use octopit_core::chat::ChatMode;
    let Some((kind, name, text)) = vctx.steps.borrow_mut().next() else {
        // Restore default Laila mode before exiting.
        win.switch_chat_mode(ChatMode::Laila);
        vctx.app.quit();
        return;
    };
    if kind == "companion" {
        win.show_page(PageId::Companion);
        let path = format!("{}/{name}.png", vctx.dir);
        let w2 = Rc::clone(&win);
        glib::timeout_add_local_once(Duration::from_millis(1200), move || {
            let w3 = Rc::clone(&w2);
            save_retry(&w2, path, Box::new(move || verify_next(w3, vctx)), 4);
        });
        return;
    }
    let mode = if kind == "assistant" {
        ChatMode::Assistant
    } else {
        ChatMode::Laila
    };
    win.show_page(PageId::Chat);
    win.switch_chat_mode(mode);
    win.send_typed_chat(&text);
    wait_and_capture(win, vctx, name, 100);
}

fn wait_and_capture(win: Rc<Window>, vctx: VerifyCtx, name: String, tries_left: u32) {
    if win.chat_busy() && tries_left > 0 {
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            wait_and_capture(win, vctx, name, tries_left - 1);
        });
        return;
    }
    if let Some((q, a, lbl)) = win.last_chat_turn() {
        println!("TURN [{name}] label={lbl:?} q={q:?} a={a:?}");
    }
    let path = format!("{}/{name}.png", vctx.dir);
    let w2 = Rc::clone(&win);
    glib::timeout_add_local_once(Duration::from_millis(400), move || {
        let w3 = Rc::clone(&w2);
        save_retry(&w2, path, Box::new(move || verify_next(w3, vctx)), 4);
    });
}
